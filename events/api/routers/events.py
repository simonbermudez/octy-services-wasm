#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.events import *
from .request_models.account import Account
from .dto.events import *
from services.events import EventsService

#python imports
from typing import Optional

#external imports
from fastapi import APIRouter, Request, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address
from pydantic import BaseModel


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Events routers:
# Events API endpoints
######################################

######################################
# Route : /v1/retention/events/create
# Request type : POST
# Required parameters : event
# Description : Create event record .
# Returns : Status of event creation.
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/events/create')
@limiter.limit("120/minute")
async def create_event_instance(request: Request, 
    event : CreateEvent,
    current_account: Account = Depends(decode_account_jwt)):
    processing_event = await EventsService(current_account).create_event(event)
    return CreateEventDTO(processing_event).dto()


######################################
# Route : /v1/retention/events/create/batch
# Request type : POST
# Required parameters : events
# Description : Create events records
# Returns : Status of event creation.
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/events/create/batch')
@limiter.limit("120/minute")
async def batch_create_event_instances(request: Request, 
    events : BatchCreateEvents,
    current_account: Account = Depends(decode_account_jwt)):
    valid_events, invalid_events = await EventsService(current_account).batch_create_events(events)
    return BatchCreateEventsDTO(valid_events, invalid_events).dto()



######################################
# Internal Events API endpoints. 
# Available via cluster IP only.
######################################

######################################
# Route : /v1/internal/events
# Request type : POST
# Required parameters : POST body : {"segment_timeframe" : 4 (days), "account_id" : ""}
# Description : Internal service used to get event instances
# Returns : Events found within specified timeframe
# NOTE : Do not expose route in ingress
######################################

@router.post('/v1/internal/events') 
async def get_events_internal(request: Request,  e : GetEventsInternal):

    # Validate pagination headers set
    cursor, pag_message = await validate_pagination_request(request, None)
    if cursor == None:
        raise OctyException(400,'Missing Parameters', [{'error_message' : pag_message, 
            'extended_help': ''}])
    events, total = await EventsService(account_id=e.account_id)\
        .get_events(timeframe=e.timeframe, 
                    cursor=cursor, 
                    event_sequence_event=e.event_sequence_event, 
                    profile_ids=e.profile_ids,
                    event_type=e.event_type)
    return InternalGetEventsDTO(events, total).dto()

######################################
# Route : /v1/internal/events/delete
# Request type : POST
# Required parameters : POST body : {"account_id" : ""}
# Description : Internal service used to delete events and event instances for a given account
# Returns : bool : True if events were deleted successfully, False otherwise
# NOTE : Do not expose route in ingress
######################################

@router.post('/v1/internal/events/delete')
async def delete_events_internal(request: Request,  e : DeleteEventsInternal):
    res = await EventsService(account_id=e.account_id).delete_account_events_internal()
    return InternalDeleteEventsDTO(res).dto()


#By Munashe
# Get latest checkout info submmited event for given checkout id
@router.post('/v1/retention/events') 
async def get_latest_checkout_info_submmited_event(request: Request,  checkout_id : str, current_account: Account = Depends(decode_account_jwt)):

    event = await EventsService(current_account)\
        .get_latest_checkout_info_submmited_event(checkout_id=checkout_id)
    return GetEventDTO(event).dto()