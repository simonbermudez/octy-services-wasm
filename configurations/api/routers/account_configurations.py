#module imports 
from .error_handlers import *
from .utils import *
from .request_models.configurations import *
from .request_models.account import Account
from .dto.configurations import *
from data.repositories.implementation.account_config_repository import accountConfigRepository

#python imports

#external imports
from fastapi import APIRouter, Request, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# ACCOUNT CONFIG
######################################

######################################
# Route : /v1/configurations/account/set
# Request type : POST
# Required parameters : configurations [raw]
# Description : Allow client to set configurations their account.
# Returns : Newley created updated account configurations
# Limits : 200 requests per day, 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.post('/v1/configurations/account/set')
@limiter.limit("120/minute")
async def set_account_configs(request: Request, 
                            configs: SetAccountConfigs, 
                            current_account: Account = Depends(decode_account_jwt)):
    configs.account_id = current_account.account_id
    await accountConfigRepository.set_account_configs(configs)

    return SetAccountConfigsDTO(configs).dto()


######################################
# Route : /v1/configurations/account
# Request type : GET
# Required parameters : null
# Description : Access all set account configurations associated with account.
# Returns : configurations object(s)
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.get('/v1/configurations/account')
@limiter.limit("120/minute")
async def get_account_configs(request: Request, current_account: Account = Depends(decode_account_jwt)):
    return AccountConfigsDTO(current_account).dto()
