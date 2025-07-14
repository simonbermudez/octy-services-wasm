# module imports
from data.repositories.Ievents_repository import EventsInterface
from utils.utils import *
from api.routers.error_handlers import *
import data.context.db_context as ctx

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td

import time

# external imports
from bson import ObjectId
from bson.json_util import dumps
from pymongo.errors import BulkWriteError
from pymongo import UpdateOne


class _EventsRepository(EventsInterface):
    """
        _EventsRepository
        Handles:
        - Creating events (single and batch)
        - Get events descending
        ...

        Attributes
        ----------
        none
    """
    def __init__(self):
        self.collection = ctx.contextManager.db["tbl_event_instances"]

    async def create_event(self, account_id: str, event: dict) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event : dict

        Returns
        ----------
        None
        """
        await self.collection.insert_one({
            "_id": event['event_id'],
            "account_id": account_id,
            "profile_id": event['profile_id'],
            "event_type_id": event['event_type_id'],
            "event_type": event['event_type'],
            "event_properties": event['event_properties'],
            "created_at": event.get("created_at", int(time.time() * 1000))
        })

    async def get_latest_checkout_info_submmited_event(self, account_id: str, checkout_id: str) -> dict:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        checkout_id : str
            checkout_id

        Returns
        ----------
        event type : dict
        """
        doc = await self.collection.find_one(
            {
                "account_id": account_id,
                "event_type": "checkout_contact_info_submitted",
                "event_properties.checkout_id": checkout_id
            },

            sort=[("created_at", -1)]
        )
        if doc:
            doc['event_type_id'] = doc['_id']
            doc.pop('_id', None)
            doc['created_at'] = int_to_dt(doc['created_at'], as_str=True)
            return doc
        return None

    async def batch_create_events(self, account_id: str, events_batch: list) -> tuple[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        events_batch : list

        Returns
        ----------
        created_events, failed_to_create : list, list
        """       
        documents = []
        failed_to_create = []
        event_ids = []

        for event in events_batch:

            event_ids.append(event['event_id'])

            documents.append({
                "_id": event['event_id'],
                "account_id": account_id,
                "profile_id": event['profile_id'],
                "event_type_id": event['event_type_id'],
                "event_type": event['event_type'],
                "event_properties": event['event_properties'],
                "created_at": event['created_at']
            })

        try:
            await self.collection.insert_many(documents, ordered=False)
        except BulkWriteError as bwe:
            for err in bwe.details.get('writeErrors', []):
                failed_to_create.append({
                    "event_id": err["op"]["_id"],
                    "error_message": f"Duplicate or invalid event: {err['op']['_id']}"
                })

        valid_ids = list(set(event_ids) - {e['event_id'] for e in failed_to_create})
        created = [e for e in events_batch if e['event_id'] in valid_ids]

        for c in created:
            c.pop("account_id", None)

        return created, failed_to_create

    async def get_events_meta(self, account_id: str, event_type_list: list) -> tuple[list, int]:
        """
        Get latest event instance of EACH provided event type in events to determine each event property required data type.
        Also get current count of total events associated with an account, to ensure account is not surpassing creation limit

        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_list : list[str]

        Returns
        ----------
        event_types : list[dict]
        event_count : int
        """
        pipeline = {
            "$facet": {
                f"query{i}": [
                    {"$match": {"account_id": account_id, "event_type": et}},
                    {"$sort": {"created_at": -1}},
                    {"$limit": 1}
                ]
                for i, et in enumerate(event_type_list)
            }
        }

        event_count = await self.collection.count_documents({"account_id": account_id})
        cursor = self.collection.aggregate([pipeline])
        docs = await cursor.to_list(length=1)

        result = []
        if docs:
            doc = docs[0]
            for key in doc:
                if doc[key]:
                    result.append({
                        "event_type": doc[key][0]["event_type"],
                        "event_properties": doc[key][0]["event_properties"]
                    })

        return result, event_count

    async def get_events(self, account_id: str, timeframe: int, cursor: int,
                         event_sequence_event: dict = None,
                         profile_ids: list = None,
                         event_type: str = None) -> tuple[list, int]:
        
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        timeframe : int
            the number of minutes events must have occurred in, since now
        cursor : int
            pagination cursor
        event_sequence_event : dict
            past segment definition event sequence event object
        profile_ids : list
            A list of octy profile identifiers
        event_type : str

        Returns
        ----------
        events : list
        total : int
        """
        from_dt = dt.now() - td(minutes=timeframe + 1)
        query = {"account_id": account_id, "created_at": {"$gt": from_dt}}

        if event_sequence_event:
            query["event_type"] = event_sequence_event["event_type"]
            for k, v in event_sequence_event.get("event_properties", {}).items():
                query[f"event_properties.{k}"] = v

        elif profile_ids and event_type:
            query["profile_id"] = {"$in": profile_ids}
            query["event_type"] = event_type

        elif profile_ids:
            query["profile_id"] = {"$in": profile_ids}

        cursor_obj = self.collection.find(query).skip(cursor).limit(3000)
        total = await self.collection.count_documents(query)
        raw = await cursor_obj.to_list(length=3000)

        found = []
        for doc in raw:
            doc["event_id"] = doc["_id"]
            doc.pop("_id", None)
            doc["created_at"] = int_to_dt(doc["created_at"], as_str=True)
            found.append(doc)

        return found, total

    async def update_events_owner(self, account_id: str, profiles: list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profiles : list
            list of parent and relative child octy profile identifiers

        Returns
        ----------
        None
        """ 
        all_child_profile_ids = []
        [all_child_profile_ids.extend(p.child_profiles) for p in profiles]

        def _child_to_parent(pid):
            profile = next((p for p in profiles if pid in p.child_profiles), None)
            return profile.parent_profile if profile else None

        operations = []
        for cpi in all_child_profile_ids:
            parent = _child_to_parent(cpi)
            if parent:
                operations.append({
                    "filter": {"account_id": account_id, "profile_id": cpi},
                    "update": {"$set": {"profile_id": parent}}
                })

        if operations:
            requests = [UpdateOne(op["filter"], op["update"]) for op in operations]
            await self.collection.bulk_write(requests, ordered=False)

    async def delete_profile_events(self, account_id: str, profile_id: str) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_id : str
            octy profile identifier

        Returns
        ----------
        None
        """
        await self.collection.delete_many({"account_id": account_id, "profile_id": profile_id})

    async def delete_account_events(self, account_id: str) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        bool
        """
        await self.collection.delete_many({"account_id": account_id})

    async def get_profile_ids(self, account_id: str, profile_ids: list) -> tuple[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            list of profile ids to verfiy existence of

        Returns
        ----------
        valid profiles : list
        invalid profiles : list
        """
        valid_profiles, invalid_profiles = [], []

        payload = {
            "account_id": account_id,
            "profiles": profile_ids,
            "get_all": False
        }
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids=true"
        session = requests_retry_session()

        try:
            response = session.post(
                url, 
                data=json.dumps(payload), 
                timeout=60
                )
            response.raise_for_status()
        except Exception as ex:
            raise Exception(f"Profile API error: {ex}")

        data = response.json()
        valid_profiles = data.get("profiles", [])
        invalid_profiles = data.get("not_found", [])
        return valid_profiles, invalid_profiles

    async def get_live_segment_definitions(self, account_id: str) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account identifier

        Returns
        ----------
        found_segments : list
        """
        url = f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments?account_id={account_id}&status=active&segment_type=live"
        session = requests_retry_session()

        try:
            response = session.get(url, timeout=60)
            response.raise_for_status()
        except Exception as ex:
            raise Exception(f"Segment API error: {ex}")

        return response.json().get("segments", [])


eventsRepository = _EventsRepository()