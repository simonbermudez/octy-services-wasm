# module imports
from .utils import *
from .request_models.account import *
from services.account import accountService
from .dto.account import *

# python imports

# external imports
from fastapi import APIRouter, Request, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address

router = APIRouter()
limiter = Limiter(key_func=get_remote_address)

######################################
# Account routers:
# Account management API endpoints
######################################


######################################
# Route : /v1/admin/account/create
# Request type : POST
# Required parameters : email address [string], account name [string], access_level [int],
#   contact_surname [string], contact_name [string], webhook_url [string], algorithm_tags [list]
# Description : Create new Octy account
# Returns : Created account object
# Limits : 120 Requests per minute
# Requires auth : YES -- Admin Public Key & Admin Secret Key
######################################

@router.post('/v1/admin/account/create',
             dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def create_new_account(request: Request, account: CreateAccount):
    new_account = await accountService.create_account(account)
    return CreateAccountDTO(new_account['account_name'],
                            new_account['contact_email_address'],
                            new_account['pk'],
                            new_account['notification_sent'],).dto()


######################################
# Internal Account API endpoints. 
# Available via cluster IP only.
######################################

######################################
# Route : /v1/internal/accounts
# Request type : POST
# Required parameters : POST body : {"account_ids" : ["account_123","account_456",...]}
# Description : Internal service used to get accounts
# Returns : Found account ids
# NOTE : Do not expose route in ingress
######################################

@router.post('/v1/internal/accounts') 
async def get_accounts_internal(request: Request,  a : GetAccountsInternal):
    
    # Validate pagination headers set
    cursor, pag_message = await validate_pagination_request(request, None)
    if cursor == None:
        raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
            'extended_help': ''}])
    accounts, total = accountService.get_accounts_internal(account_ids=a.account_ids, cursor=cursor)
    return GetAccountsInternalDTO(accounts, total).dto()
