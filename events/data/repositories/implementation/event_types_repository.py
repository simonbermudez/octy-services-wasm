# module imports
from data.repositories.Ievent_types_repository import EventTypesInterface
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

class _EventTypesRepository(EventTypesInterface):
    def __init__(self):
        self.collection = lambda: ctx.contextManager.db["tbl_custom_event_types"]

    async def get_event_types_count(self, account_id: str):
        return await self.collection.count_documents({"account_id": account_id})

    async def get_event_type_by_ids(self, account_id: str, event_type_ids: list) -> list:
        query = {
            "$and": [
                {"event_type_id": {"$in": event_type_ids}},
                {"account_id": account_id}
            ]
        }
        cursor = self.collection.find(query)
        docs = await cursor.to_list(length=None)
        for doc in docs:
            doc["event_type_id"] = doc["_id"]
            _format_event_type(doc)
        return docs

    async def get_event_type_by_name(self, account_id: str, event_type: str) -> dict:
        doc = await self.collection.find_one({"account_id": account_id, "event_type": event_type})
        if doc:
            doc["event_type_id"] = doc["_id"]
            return _format_event_type(doc)
        return None

    async def get_event_types_by_name(self, account_id: str, event_type_names: list) -> tuple[list, list]:
        found_event_types = []
        not_found = []

        cursor = self.collection.find({
            "account_id": account_id,
            "event_type": {"$in": event_type_names}
        })
        docs = await cursor.to_list(length=None)

        for doc in docs:
            doc["profile_id"] = doc["_id"]
            found_event_types.append(_format_event_type(doc))

        for name in event_type_names:
            if not any(et["event_type"] == name for et in found_event_types):
                not_found.append(name)

        return found_event_types, not_found

    async def get_all_event_types(self, account_id: str, cursor: int) -> tuple[list, int]:
        cursor_data = self.collection.find({"account_id": account_id}).skip(cursor).limit(100)
        docs = await cursor_data.to_list(length=100)
        total = await self.collection.count_documents({"account_id": account_id})
        for doc in docs:
            doc["event_type_id"] = doc["_id"]
            _format_event_type(doc)
        return docs, total

    async def create_event_types(self, event_types: list) -> tuple[list, list]:
        failed_to_create = []
        valid_docs = []
        event_type_ids = []

        for et in event_types:
            if et['event_type'] in Config['SYSTEM_EVENT_TYPES']:
                failed_to_create.append({
                    'event_type': et['event_type'],
                    'error_message': f'System event type exists: {et["event_type"]}'
                })
                continue

            valid_docs.append({
                "_id": et['event_type_id'],
                "account_id": et['account_id'],
                "event_type": et['event_type'],
                "event_properties": et['event_properties'],
                "created_at": et.get('created_at') or int(time.time() * 1000)
            })
            event_type_ids.append(et['event_type'])

        try:
            if valid_docs:
                await self.collection.insert_many(valid_docs, ordered=False)
        except Exception as e:
            # Check for duplicate errors
            from pymongo.errors import BulkWriteError
            if isinstance(e, BulkWriteError):
                for err in e.details.get('writeErrors', []):
                    conflict_type = err['op']['event_type']
                    failed_to_create.append({
                        'event_type': conflict_type,
                        'error_message': f'Conflict: {conflict_type}'
                    })

        created = []
        for et in event_types:
            if not any(f['event_type'] == et['event_type'] for f in failed_to_create):
                copy_et = et.copy()
                copy_et.pop('account_id', None)
                created.append(copy_et)

        return created, failed_to_create

    async def delete_event_types(self, event_types_batch: list) -> tuple[list, list]:
        deleted_event_types = []
        failed_to_delete = []

        for et in event_types_batch:
            result = await self.collection.delete_one({
                "_id": et["event_type_id"],
                "account_id": et["account_id"]
            })
            if result.deleted_count > 0:
                deleted_event_types.append({"event_type_id": et["event_type_id"]})
            else:
                failed_to_delete.append({
                    "event_type_id": et["event_type_id"],
                    "error_message": "No match found for deletion"
                })
        return deleted_event_types, failed_to_delete

    async def delete_all_event_types_by_account(self, account_id: str) -> tuple[list, list]:
        deleted = []
        failed = []

        cursor = self.collection.find({"account_id": account_id})
        docs = await cursor.to_list(length=None)
        if not docs:
            return [], [{
                "account_id": account_id,
                "error_message": f"No event types found for account_id: {account_id}"
            }]

        for doc in docs:
            deleted.append({"event_type_id": doc["_id"], "event_type": doc.get("event_type", "unknown")})

        try:
            await self.collection.delete_many({"account_id": account_id})
        except Exception as e:
            return [], [{
                "account_id": account_id,
                "error_message": f"Failed to delete: {str(e)}"
            }]

        return deleted, failed

    async def delete_account_event_types(self, account_id: str) -> bool:
        result = await self.collection.delete_many({"account_id": account_id})
        return result.deleted_count > 0

eventTypesRepository = _EventTypesRepository()

def _format_event_type(event_type : dict):
    '''
        Format event type object
    '''
    event_type.pop('_id', None)
    event_type.pop('account_id', None)
    event_type['created_at'] = int_to_dt(event_type['created_at']['$date'], as_str=True) if event_type['created_at'] != None else None
 
    return event_type