#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.configurations import *
from .request_models.account import Account
from .dto.configurations import *
from data.repositories.implementation.algorithm_config_repository import algorithmConfigRepository

#python imports
from typing import Optional, List

#external imports
from fastapi import APIRouter, Request, Depends
from fastapi.exceptions import RequestValidationError
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# ML ALGORITHM CONFIG
######################################

######################################
# Route : /v1/configurations/retention/algorithms/set
# Request type : POST
# Required parameters : algorithm_name [string], configurations [object]
# Description : Allow client to set configurations on each octy ML algorithm.
# Returns : Newley created updated algorithm configurations
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.post('/v1/configurations/retention/algorithms/set')
@limiter.limit("120/minute")
async def set_algorithm_configs(request: Request, 
                                configs: BaseSetAlgoConfigs, 
                                current_account: Account = Depends(decode_account_jwt)):

    # Algortihm specific validations
    if configs.algorithm_name == 'rec':
        # validate that provided item_id_stop_list ids are in items
        items = await algorithmConfigRepository.get_items(current_account.account_id)

        try:
            configurations = SetRecAlgoConfigs(algorithm_name=configs.algorithm_name,
                                                configurations=request._json['configurations'])
        except Exception as e:
            raise RequestValidationError(e)

        valid_stop_list=[]
        for item_id in configurations.configurations.item_id_stop_list:
            i = next((iid for iid in items if iid == item_id), None)
            if i:
                valid_stop_list.append(
                    {
                        'item_id' : i
                    }
                )
            else:
                continue
        configurations.configurations.item_id_stop_list = valid_stop_list

        return_configs = json.loads(configurations.configurations.json())
        return_configs.pop('rec_item_identifier', None)


    elif configs.algorithm_name == 'churn':
        try:
            configurations =  SetChurnAlgoConfigs(algorithm_name=configs.algorithm_name,
                                                    configurations=request._json['configurations'])
        except Exception as e:
            raise RequestValidationError(e)
        return_configs = json.loads(configurations.configurations.json())
        return_configs.pop('churn_item_identifier', None)

    return_configs.pop('event_type', None)
    configurations.account_id = current_account.account_id

    await algorithmConfigRepository.set_algorithm_configs(configurations)

    return SetAlgorithmConfigsDTO(configs.algorithm_name, return_configs).dto()


######################################
# Route : /v1/configurations/retention/algorithms
# Request type : GET
# Required parameters : null
# Description : Access all set algorithm configurations associated with account.
# Returns : configurations object(s)
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.get('/v1/configurations/retention/algorithms')
@limiter.limit("120/minute")
async def get_algorithm_configs(request: Request, current_account: Account = Depends(decode_account_jwt)):
    return AlgorithmConfigsDTO(current_account).dto()