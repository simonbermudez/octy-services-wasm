# module imports
from data.repositories.implementation.event_types_repository import eventTypesRepository
from api.routers.request_models.event_types import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
import json

# external imports
from fastapi import Request


class EventTypesService():
    """
        EventTypesService
        Handles:
        - Get event types
        - Event type creation
        - Delete event types
        ...

        Attributes
        ----------
        account : Octy account
    """
    def __init__(self, account : Account): 
        self.account = account

    def get_event_types(self,
                    event_type_ids : list = None, 
                    cursor : int = None) -> Union[dict, int]: 
        """
        Parameters
        ----------
        event_type_ids : list
            event_type_id(s)
        cursor : int
            Pagination cursor

        Returns
        ----------
        event types : dict
        total : int
        """

        if event_type_ids != None and cursor == 0:
            event_types =  eventTypesRepository.get_event_type_by_ids(account_id=self.account.account_id, event_type_ids=event_type_ids)
            count = len(event_types)
            if count<1:
                raise OctyException(400, 'Invalid event type identifier provided', 
                [{'message' : 'No custom event types were found with the provided event_type_id', 
                'extended_help': Config['CUSTOM_EVENTS_EXTENDED_HELP']}])
            
            return event_types, count
            

        elif event_type_ids == None and cursor != None:
            
            event_types, total = eventTypesRepository.get_all_event_types(account_id=self.account.account_id, cursor=cursor)
            if len(event_types)<1:
                raise OctyException(400, 'No custom event types found', 
                [{'message' : 'No custom event types found with the provided query parameters or pagination cursor exhausted', 
                'extended_help': Config['CUSTOM_EVENTS_EXTENDED_HELP']}])
            return event_types, total

    def create_event_types(self, event_types : CreateEventTypes) -> Union[list, list]:
        """
        Parameters
        ----------
        event_types : CreateEventTypes
            CreateEventTypes request model instance

        Returns
        ----------
        Created and failed to create event types : Union[list, list]
        """

        # assess allowed limits
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              eventTypesRepository.get_event_types_count(self.account.account_id),
                              len(event_types.event_types), resource_key=2)
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'message' : f'This request could not be completed as the number of event types sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} event types.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        event_type_batch = []
        for event_type in event_types.event_types:
            event_type_batch.append(
                {
                    'event_type_id' : generate_uid('custom_event_type'),
                    'account_id' : self.account.account_id,
                    'event_type' : event_type.event_type,
                    'event_properties' : event_type.event_properties
                }
            )

        created, failed = eventTypesRepository.create_event_types(event_type_batch)

        if len(created) < 1:
            raise OctyException(400, 'No event types created!', failed)

        return created, failed


    def delete_event_types(self, event_type_ids : DeleteEventTypes) -> Union[list, list]:
        """
        Parameters
        ----------
        event_type_ids : DeleteEventTypes
            DeleteEventTypes request model instance
    
        Returns
        ----------
        Deleted and failed to delete event type ids : Union[list, list]
        """
        event_type_id_batch=[]
        for et in event_type_ids.event_type_ids:
            event_type_id_batch.append({
                "event_type_id" : et,
                "account_id" : self.account.account_id
            })

        deleted , failed = eventTypesRepository.delete_event_types(event_type_id_batch)

        if len(deleted) < 1:
            raise OctyException(400, 'No event types deleted!', failed)
        return deleted, failed

    

    def get_event_types_internal(self, account_id : str, event_type_names : list) -> Union[list, list]: 
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_names : list
            custom event type names

        Returns
        ----------
        event types : list
        not found event types : list
        """

        found_event_types, not_found =  eventTypesRepository.get_event_types_by_name(account_id=account_id, 
                                                                                  event_type_names=event_type_names)
        if len(found_event_types)<1:
            raise OctyException(400, 'None found!', 
            [{'message' : 'No custom event types were found with the provided event type names', 
            'extended_help': ''}])
        
        return found_event_types, not_found