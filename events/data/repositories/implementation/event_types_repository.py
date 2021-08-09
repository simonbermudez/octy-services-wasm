# module imports
from data.repositories.Ievent_types_repository import EventTypesInterface
from data.models.db_schemas import tbl_custom_event_types
from utils.utils import *
from api.routers.error_handlers import *


# python imports
from typing import *
import json
from datetime import datetime as dt
import time

# external imports
from mongoengine.errors import NotUniqueError, DoesNotExist, BulkWriteError
from mongoengine.queryset.visitor import Q
from pymongo.errors import BulkWriteError, OperationFailure
from bson.json_util import dumps


class _EventTypesRepository(EventTypesInterface):
    """
        _EventTypesRepository
        Handles:
        - Retrieving event types
        - Creating event types
        - Deleting event types
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass


    def get_event_types_count(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        return tbl_custom_event_types.objects(account_id__exact=account_id).count()

    def get_event_type_by_id(self, account_id : str, event_type_id : str) -> dict:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_id : str
            The event_type_id of the event type that should be returned.

        Returns
        ----------
        result : dict
        """
        event_types = tbl_custom_event_types.objects((Q(event_type_id__exact=event_type_id) & Q(account_id__exact=account_id)))
        if event_types:
            event_type_dict = json.loads(event_types.to_json())
            event_type_dict[0]['event_type_id'] = event_type_dict[0]['_id']
            event_type_dict= _format_event_type(event_type_dict[0])
            return event_type_dict
        return None
    
    def get_event_type_by_name(self, account_id : str, event_type : str) -> dict:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type : str
            The event_type name of the event type that should be returned.

        Returns
        ----------
        result : dict
        """
        event_types = tbl_custom_event_types.objects((Q(event_type__exact=event_type) & Q(account_id__exact=account_id)))
        if event_types:
            event_type_dict = json.loads(event_types.to_json())
            event_type_dict[0]['event_type_id'] = event_type_dict[0]['_id']
            event_type_dict= _format_event_type(event_type_dict[0])
            return event_type_dict
        return None
    
    def get_event_types_by_name(self, account_id : str, event_type_names : list) -> Union[list, list]:
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

        event_types = tbl_custom_event_types.objects((Q(event_type__in=event_type_names) & Q(account_id__exact=account_id)))

        for event_type in event_types:
            event_type_dict = json.loads(event_type.to_json())
            event_type_dict['profile_id'] = event_type_dict['_id']
            event_type_dict= _format_event_type(event_type_dict)
            found_event_types.append(event_type_dict)
        
        # get all not found ids
        for etm in event_type_names:
            exists=next((key for key in found_event_types if key['event_type'] == etm), None)
            if not exists:
                not_found.append(etm)
        
        return found_event_types, not_found

    def get_all_event_types(self, account_id : str, cursor : int) -> Union[dict, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int

        Returns
        ----------
        results : dict
        total : int
        """
        event_type_dict={}
        total=0
        event_types = tbl_custom_event_types.objects(account_id__exact=account_id).skip(cursor).limit(100)
        if event_types:
            total += tbl_custom_event_types.objects(account_id__exact=account_id).count()
            event_type_dict = json.loads(event_types.to_json())
        
        #format items
        for event_type in event_type_dict:
            event_type['event_type_id'] = event_type['_id']
            _format_event_type(event_type)
        return event_type_dict, total

    def create_event_types(self, event_types : list) -> Union[list, list]:
        """
        Parameters
        ----------
        event_types : list

        Returns
        ----------
        created_event_types, failed_to_create event_types
        """
        failed_to_create=[]
        event_type_instances = []
        event_type_ids = []
        for event_type in event_types:

            if event_type['event_type'] in Config['SYSTEM_EVENT_TYPES']:
                failed_to_create.append(
                    {
                        'event_type': event_type['event_type'],
                        'error_message' : f'A system event type exists with provided event_type : {event_type["event_type"]}.'
                    }
                )
                continue

            event_type_instances.append(
                tbl_custom_event_types(
                    event_type_id=event_type['event_type_id'],
                    account_id=event_type['account_id'],
                    event_type=event_type['event_type'],
                    event_properties=event_type['event_properties']
                )
            )
            event_type_ids.append(event_type['event_type'])

        #BULK WRITE OPERATION
        invalid=[]
        bulk_operation = tbl_custom_event_types._get_collection().initialize_unordered_bulk_op()
        for event_type in event_type_instances:
            bulk_operation.insert(event_type.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                invalid.append(err['op'].to_dict()['event_type'])

        valid = list(set(event_type_ids) - set(invalid))


        for in_ in invalid:
            failed_to_create.append(
                {
                    'event_type': in_,
                    'error_message' : f'Another custom event type exists with provided event_type : {in_}'
                }
            )
        created_event_types=[]
        for v in valid:
            et=next((d for i,d in enumerate(event_types) if v == d['event_type']),None)
            if et:
                et.pop('account_id', None)
                created_event_types.append(et)
        
        return created_event_types, failed_to_create

    def delete_event_types(self, event_types_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        event_types_batch : list[str]

        Returns
        ----------
        deleted_event_types : list
        failed_to_delete : list
        """
        deleted_event_types=[]
        failed_to_delete=[]
        event_type_ids=[]

        for event_type in event_types_batch:
            event_type_ids.append(event_type['event_type_id'])


        event_types = json.loads(tbl_custom_event_types.objects(event_type_id__in=event_type_ids).to_json())
        if not event_types:
            for event_type in event_types_batch:
                failed_to_delete.append(
                    {
                        'event_type_id' : event_type['event_type_id'],
                        'error_message' : f'No event type found with event_type_id : {event_type["event_type_id"]}'
                    }
                )
            return deleted_event_types, failed_to_delete

        
        
        bulk_operation = tbl_custom_event_types._get_collection().initialize_unordered_bulk_op()
        for event_type in event_types_batch:
            et_object=next((key for key in event_types if key['_id'] == event_type['event_type_id'] and key['account_id'] == event_type['account_id']), None)
            if et_object:
                deleted_event_types.append(
                    {
                        'event_type_id': et_object['_id']
                    }
                )
            else:
                failed_to_delete.append(
                    {
                        'event_type_id' : event_type['event_type_id'],
                        'error_message' : f'No event type found with event_type_id : {event_type["event_type_id"]}'
                    }
                )

            bulk_operation.find({
                '$and' : [
                    {  "_id" : { "$eq" : event_type['event_type_id'] }  },
                    {  "account_id" : { "$eq" : event_type['account_id'] }  }
                ]
            }).remove()

        bulk_operation.execute()

        return deleted_event_types, failed_to_delete


eventTypesRepository = _EventTypesRepository()

def _format_event_type(event_type : dict):
    '''
        Format event type object
    '''
    event_type.pop('_id', None)
    event_type.pop('account_id', None)
    event_type['created_at'] = int_to_dt(event_type['created_at']['$date'], as_str=True) if event_type['created_at'] != None else None
 
    return event_type