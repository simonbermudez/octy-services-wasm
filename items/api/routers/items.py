#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.items import *
from services.items import ItemsService
from .dto.items import *

#python imports
from typing import Optional, List

#external imports
from fastapi import APIRouter, Request, Query, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address
from pydantic import BaseModel


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Items routers:
# Item API endpoints
######################################


######################################
# Route : /v1/retention/items/<item_id> (optional)
# Request type : GET
# Required parameters : id (item_id -- optional)
# Description : Access all items associated with account.
# Returns : item object(s)
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/items')
@limiter.limit("120/minute")
async def get_items(request: Request, 
    id : Optional[str] = None,
    current_account: Account = Depends(decode_account_jwt)):
    
    cursor = 0
    if id == None:
        # Validate pagination headers set
        cursor, pag_message = await validate_pagination_request(request,id)
        if cursor == None:
            raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
                'extended_help': Config['ITEMS_EXTENDED_HELP']}])
    
    items, total = ItemsService(current_account).get_items(id_=id,cursor=cursor)

    return GetItemsDTO(items, total, cursor).dto()


######################################
# Route : /v1/retention/items/create
# Request type : POST
# Required parameters : items list (item_id [string], item_category [string], item_name [string], item_description [string], item_price [string])
# Description : Allow client to create new items.
# Returns : Status of item creation, each item_id
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/items/create')
@limiter.limit("120/minute")
async def create_items(request: Request, 
    create_items : CreateItems,
    current_account: Account = Depends(decode_account_jwt)):
    created, failed = ItemsService(current_account).create_items(create_items)
    return CreateItemsDTO(created, failed).dto()


######################################
# Route : /v1/retention/items/update
# Request type : POST
# Required parameters : items list (item_id [string], item_category [string], item_name [string], item_description [string], item_price [string])
# Description : Allow client to update existing items.
# Returns : List of updated items / not found items
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/items/update')
@limiter.limit("120/minute")
async def update_items(request: Request, 
    update_items : UpdateItems,
    current_account: Account = Depends(decode_account_jwt)):
    updated, failed = ItemsService(current_account).update_items(update_items)
    return UpdateItemsDTO(updated, failed).dto()


######################################
# Route : /v1/retention/items/delete
# Request type : POST
# Required parameters : item list (item_id [string])
# Description : Allow client to delete existing items.
# Returns : Status of items deletion
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/items/delete')
@limiter.limit("120/minute")
async def delete_items(request: Request, 
    delete_items : DeleteItems,
    current_account: Account = Depends(decode_account_jwt)):
    delete, failed = await ItemsService(current_account).delete_items(delete_items)
    return DeleteItemsDTO(delete, failed).dto()


######################################
# Internal Items routers:
# Internal Item API endpoints. 
# Available via cluster IP only.
######################################

######################################
# Route : /v1/internal/items
# Request type : GET
# Required parameters : account_id : Octy account_id str ,ids (optional) return ids only #?ids=true, status
# Description : Internal service used to get items
# Returns : Items
# NOTE : Do not expose route in ingress
######################################

@router.get('/v1/internal/items') 
async def get_items_internal(request: Request,  account_id : str, ids : bool, status : str):

    # Validate pagination headers set
    cursor, pag_message = await validate_pagination_request(request, None)
    if cursor == None:
        raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
            'extended_help': Config['ITEMS_EXTENDED_HELP']}])
    
    items, total = ItemsService(None).get_items_internal(account_id=account_id,cursor=cursor, ids=bool(ids), status=status)

    return GetItemsDTO(items, total, cursor).dto()