#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.event_types import *
from .request_models.account import Account
from .dto.event_types import *
from services.event_types import EventTypesService

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
# Custom event types routers:
# Custom event types API endpoints
######################################

######################################
# Route : /v1/retention/events/types
# Request type : GET
# Required parameters : ?id (optional)
# Description : Access all custom event types associated with account.
# Returns : event types
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/events/types')
@limiter.limit("120/minute")
async def get_custom_event_types(request: Request,
    id : Optional[str] = None,
    current_account: Account = Depends(decode_account_jwt)):

    cursor = 0
    if id == None:
        # Validate pagination headers set
        cursor, pag_message = await validate_pagination_request(request,id)
        if cursor == None:
            raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
                'extended_help': Config['CUSTOM_EVENTS_EXTENDED_HELP']}])

    event_types, total = EventTypesService(current_account).get_event_types(id_=id, cursor=cursor)


    return GetEventTypesDTO(event_types,total, cursor).dto()


######################################
# Route : /v1/retention/events/types/create
# Request type : POST
# Required parameters : events list(event_type [string], event_properties [object])
# Description : Creates custom event type
# Returns : Summary of newley created custom event types
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.post('/v1/retention/events/types/create')
@limiter.limit("120/minute")
async def create_custom_event_types(request: Request, 
    event_types : CreateEventTypes,
    current_account: Account = Depends(decode_account_jwt)):
    created, failed = EventTypesService(current_account).create_event_types(event_types)
    return CreateEventTypesDTO(created, failed).dto()

######################################
# Route : /v1/retention/events/types/delete
# Request type : POST
# Required parameters : events list(event_type [string])
# Description : Deletes custom event types
# Returns : Status of custom event type deleteion
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.post('/v1/retention/events/types/delete')
@limiter.limit("120/minute")
async def delete_custom_event_types(request: Request, 
    event_type_ids : DeleteEventTypes,
    current_account: Account = Depends(decode_account_jwt)):
    delete_event_types, failed = EventTypesService(current_account).delete_event_types(event_type_ids)
    return DeleteEventTypesDTO(delete_event_types, failed).dto()


######################################
# Route : /v1/internal/events/types
# Request type : POST
# Required parameters : POST body : {"event_types" : ["login", "logout", "other event type" ...]}
# Description : Internal service used to get custom event types
# Returns : custom event types, not found custom event types ids
# NOTE : Do not expose route in ingress
######################################

@router.post('/v1/internal/events/types') 
async def get_profiles_internal(request: Request,  event_type_names : GetEventTypesInternal):


    # do not allow more than 200 event types
    if len(event_type_names.event_type_names) > 200:
        raise OctyException(400,'Exceeded resource request limit', [{'message' : 'can only get 200 event_type_names per request', 
            'extended_help': ''}])

    found_event_types, not_found = EventTypesService(None).get_event_types_internal(account_id=event_type_names.account_id, event_type_names=event_type_names.event_type_names)
    
    return GetEventTypesInternalDTO(found_event_types, not_found).dto()