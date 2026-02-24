# module imports
from data.repositories.Isegmentation_repository import SegmentationInterface
from utils.utils import *
from api.routers.error_handlers import *
import data.context.db_context as ctx
from config import Config

# python imports
from typing import *
import json
from datetime import datetime as dt
import time

# external imports
from pymongo.errors import BulkWriteError, OperationFailure
from bson.json_util import dumps
from bson import ObjectId


class _SegmentationRepository(SegmentationInterface):
    """
        _SegmentationRepository
        Handles:
        - Get Segment definitions
        - Segment definitions creation
        - Delete Segment definitions

        ...

        Attributes
        ----------
        ...
    """
    def __init__(self):
        self.collection = lambda: ctx.contextManager.db["tbl_segments"]

    async def get_segment_count(self, account_id: str) -> int:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        return await self.collection().count_documents({"account_id": account_id})

    async def get_segment_by_identifiers(self, identifiers: list, account_id: str) -> Union[list, int]:
        """
        Parameters
        ----------
        identifiers : list
            Segment identifier(s)
        account_id : str
            Octy account id

        Returns
        ----------
        segments : list
        total : int
        """
        query = {
            "$and": [
                {"account_id": account_id},
                {"status": "active"}
            ]
        }

        if identifiers is not None:
            query["$and"].append({
                "$or": [
                    {"_id": {"$in": identifiers}},
                    {"segment_name": {"$in": identifiers}}
                ]
            })

        cursor = self.collection().find(query).skip(0).limit(100)
        docs = await cursor.to_list(length=100)
        total = await self.collection().count_documents(query)
        
        # Format segments
        for segment in docs:
            segment['segment_id'] = segment['_id']
            _format_segment(segment)

        return docs, total

    async def get_segment_by_attr(self, account_id: str, segment: dict) -> dict:
        """
        Parameters
        ----------
        account_id : str
        segment : dict

        Returns
        ----------
        segment_dict : dict
        """
        # First try to find by segment name
        segment_dict = await self.collection().find_one({
            "segment_name": segment.get("segment_name"),
            "account_id": account_id
        })
        
        if segment_dict:
            segment_dict['segment_id'] = segment_dict['_id']
            segment_dict = _format_segment(segment_dict)
            return segment_dict

        # Build query based on profile properties
        query = {
            "account_id": account_id,
            "segment_type": segment.get("segment_type"),
            "segment_sub_type": segment.get("segment_sub_type"),
            "status": "active"
        }

        if segment.get("profile_property_name") and segment.get("profile_property_value"):
            query["profile_property_name"] = segment.get("profile_property_name")
            query["profile_property_value"] = segment.get("profile_property_value")

        cursor = self.collection().find(query)
        docs = await cursor.to_list(length=None)
        
        # Convert event_sequence to comparable format
        es_json_list = []
        for es in segment.get("event_sequence", []):
            if hasattr(es, 'dict'):
                es_json_list.append(es.dict())
            else:
                es_json_list.append(es)

        for found_segment in docs:
            segment_es_list = found_segment.get("event_sequence", [])
            
            if segment_es_list == es_json_list:
                if (found_segment.get("segment_type") == segment.get("segment_type") and
                    found_segment.get("segment_sub_type") == segment.get("segment_sub_type") and
                    found_segment.get("segment_timeframe") == segment.get("segment_timeframe") and
                    found_segment.get("profile_property_name") == segment.get("profile_property_name") and
                    found_segment.get("profile_property_value") == segment.get("profile_property_value")):
                    
                    found_segment['segment_id'] = found_segment['_id']
                    found_segment = _format_segment(found_segment)
                    return found_segment

        return None

    async def get_past_segments_by_profile_ids(self, account_id: str, profile_ids: list) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        segments : list
        """
        query = {
            "$and": [
                {"account_id": account_id},
                {"segment_type": "past"},
                {"status": "active"},
                {"profile_ids": {"$in": profile_ids}}
            ]
        }
        
        cursor = self.collection().find(query)
        segments = await cursor.to_list(length=None)
        return segments

    async def get_segments(self, account_id: str, segment_type: str, status: str, cursor: int, internal: bool = False) -> Union[list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_type : str
            live, past or all
        status : str
            the status to filter results by
        cursor : int
            Pagination cursor
        internal : bool
        Returns
        ----------
        found_segments, total : list, int
        """
        query = {"account_id": account_id}
        
        if status != 'all':
            query["status"] = status
            
        if segment_type != 'all':
            query["segment_type"] = segment_type

        cursor_data = self.collection().find(query).skip(cursor).limit(100)
        docs = await cursor_data.to_list(length=100)
        total = await self.collection().count_documents(query)
        
        found_segments = []
        for segment in docs:
            segment['segment_id'] = segment['_id']
            segment = _format_segment(segment, internal=internal)
            found_segments.append(segment)
            
        return found_segments, total

    async def create_segment(self, segment: dict) -> None:
        """
        Parameters
        ----------
        segment : dict
            Segment data

        Returns
        ----------
        None
        """
        # Format event sequence
        event_sequence = []
        for es in segment.get('event_sequence', []):
            if hasattr(es, 'dict'):
                event_sequence.append(es.dict())
            else:
                event_sequence.append({
                    'event_type': es.get('event_type'),
                    'exp_timeframe': es.get('exp_timeframe'),
                    'action_inaction': es.get('action_inaction'),
                    'event_properties': es.get('event_properties')
                })

        new_segment = {
            "_id": segment['segment_id'],
            "account_id": segment['account_id'],
            "segment_name": segment['segment_name'],
            "segment_type": segment['segment_type'],
            "segment_sub_type": segment['segment_sub_type'],
            "segment_timeframe": segment['segment_timeframe'],
            "event_sequence": event_sequence,
            "profile_property_name": segment.get('profile_property_name'),
            "profile_property_value": segment.get('profile_property_value'),
            "status": "active",
            "created_at": int(time.time() * 1000),
            "profile_ids": []
        }

        await self.collection().insert_one(new_segment)

    async def update_segment_status(self, account_id: str, segment_ids: list, status: str) -> Union[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers
        status : str
            the status to filter results by
        Returns
        ----------
        pending_deleted_segments : List
        failed_to_delete : List
        """
        pending_deleted_segments = []
        failed_to_update = []

        # Bulk update operation
        bulk_ops = []
        for s in segment_ids:
            bulk_ops.append({
                "update_one": {
                    "filter": {
                        "$and": [
                            {"_id": s['segment_id']},
                            {"account_id": account_id}
                        ]
                    },
                    "update": {"$set": {"status": status}}
                }
            })

        try:
            if bulk_ops:
                result = await self.collection().bulk_write(bulk_ops, ordered=False)
                # Add successfully updated segments to pending list
                for s in segment_ids:
                    pending_deleted_segments.append(s)
        except BulkWriteError as bwe:
            for err in bwe.details.get('writeErrors', []):
                segment_id = err.get('op', {}).get('u', {}).get('$set', {}).get('segment_id')
                mes = f"Unknown error occurred when updating segment with segment_id : {segment_id}"
                failed_to_update.append({
                    'segment_id': segment_id,
                    'error_message': mes
                })
                # Remove from pending list
                pending_deleted_segments = [i for i in pending_deleted_segments if i['segment_id'] != segment_id]

        return pending_deleted_segments, failed_to_update

    async def update_past_segment_profile_ids(self, account_id: str, segment_id: str, profile_ids: list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_id : str
            segment identifier
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        None
        """
        try:
            await self.collection().update_one(
                {
                    "account_id": account_id,
                    "segment_id": segment_id
                },
                {"$set": {"profile_ids": profile_ids}}
            )
        except OperationFailure as e:
            raise Exception(f"[toxic]:: {e}")

    async def delete_segments(self, account_id: str, segment_ids: list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers

        Returns
        ----------
        None
        """
        bulk_ops = []
        for segment in segment_ids:
            bulk_ops.append({
                "delete_one": {
                    "filter": {
                        "$and": [
                            {"_id": segment['segment_id']},
                            {"account_id": account_id}
                        ]
                    }
                }
            })

        if bulk_ops:
            await self.collection().bulk_write(bulk_ops, ordered=False)

    async def get_event_types_by_name(self, account_id: str, event_type_names: list) -> Union[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_names : list
            list of event_type names

        Returns
        ----------
        result : list, list
        """
        found_event_types = []
        not_found = []

        # if none supplied return empty lists
        if len(event_type_names) < 1:
            return found_event_types, not_found

        payload = {
            'account_id': account_id,
            'event_type_names': event_type_names
        }
        url = f"{Config['EVENT_SERVICE_CLUSTER_IP']}/v1/internal/events/types"
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                data=json.dumps(payload),
                timeout=60
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')

        if response.status_code == 400:
            return found_event_types, event_type_names

        body = json.loads(response.text)
        for et in body['event_types']:
            found_event_types.append(et)

        for ivet in body['not_found']:
            not_found.append(ivet)

        return found_event_types, not_found

    async def delete_account_segments(self, account_id: str) -> bool:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        None
        """
        result = await self.collection().delete_many({"account_id": account_id})
        return result.deleted_count > 0


segmentationRepository = _SegmentationRepository()


def _format_segment(segment: dict, internal: bool = False) -> dict:
    '''
        Format segment objects
    '''
    # Ensure Event Property keys is supplied with each event sequence object
    for event in segment.get('event_sequence', []):
        if 'event_properties' not in event:
            event['event_properties'] = None

    if not internal:
        segment.pop('account_id', None)
        segment.pop('_id', None)
        segment.pop('id', None)

        if segment.get('segment_sub_type', 0) < 3:
            segment.pop('profile_property_name', None)
            segment.pop('profile_property_value', None)

        try:
            created_at = segment.get('created_at')
            if created_at:
                if isinstance(created_at, dict) and '$date' in created_at:
                    segment['created_at'] = int_to_dt(created_at['$date'], as_str=True)
                else:
                    segment['created_at'] = int_to_dt(created_at, as_str=True)
            else:
                segment['created_at'] = None
        except (KeyError, TypeError):
            pass
            
        segment['profile_count'] = len(segment.get('profile_ids', []))
        segment.pop('profile_ids', None)
        return segment
    
    segment.pop('_id', None)
    return segment