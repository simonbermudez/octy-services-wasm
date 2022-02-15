#module imports 
from .error_handlers import *
from .utils import *
from .request_models.billing import *
from services.billing import BillingService
from .dto.billing import *
from config import Config

#python imports
from typing import Optional
import re

#external imports
from fastapi import APIRouter, Request
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# billing routers:
# billing API endpoints
######################################

######################################
# Route : /v1/admin/billing/units
# Request type : GET
# Required parameters : (listed below)
# Description : Return billiable units based on provided filter parameters.
# Returns : filtered billable units
# Limits : --
# Requires auth : YES -- Admin Public Key & Admin Secret Key
######################################

@router.get('/v1/admin/billing/units')
async def get_billable_units(request : Request, 
                    account_ids : Optional[str] = None, 
                    account_types : Optional[str] = None, 
                    unit_types : Optional[str] = None, 
                    metrics : Optional[str] = None, 
                    process_names : Optional[str] = None,
                    cost_upper_range : Optional[int] = None, 
                    cost_lower_range : Optional[int] = None,
                    currencies : Optional[str] = None,
                    created_at_upper_range : Optional[str] = None,
                    created_at_lower_range : Optional[str] = None):

    cursor, pag_message = await validate_pagination_request(request)
    if cursor == False:
        raise OctyException(400,'Missing Parameters', [{'error_message' : pag_message, 
            'extended_help': ''}])
    
    def _str_to_list(str_l) -> list or None:
        if str_l:
            str_params = re.sub(r'(\s|\u180B|\u200B|\u200C|\u200D|\u2060|\uFEFF)+', '', str_l)
            params = str_params.split(",")
            params = list(dict.fromkeys(filter(None, params)))
            return params
        return None

    b = BillingService()
    units, total =  await b.get_billable_units(
                    _str_to_list(account_ids), 
                    _str_to_list(account_types), 
                    _str_to_list(unit_types), 
                    _str_to_list(metrics), 
                    _str_to_list(process_names),
                    cost_upper_range, 
                    cost_lower_range, 
                    _str_to_list(currencies),
                    created_at_upper_range, 
                    created_at_lower_range,
                    int(cursor))
    return GetBillableUnitsDTO(units, total, int(cursor)).dto()


######################################
# Route : /v1/admin/billing/units
# Request type : GET
# Required parameters : (listed below)
# Description : Return subscription plans based on provided filter parameters.
# Returns : filtered subscription plans
# Limits : --
# Requires auth : YES -- Admin Public Key & Admin Secret Key
######################################

@router.get('/v1/admin/billing/subscriptions')
async def get_subscription_plans(request : Request,
                    account_types : Optional[str] = None):

    def _str_to_list(str_l) -> list or None:
        if str_l:
            str_params = re.sub(r'(\s|\u180B|\u200B|\u200C|\u200D|\u2060|\uFEFF)+', '', str_l)
            params = str_params.split(",")
            params = list(dict.fromkeys(filter(None, params)))
            return params
        return None

    subscriptions = list()
    if account_types:
        for account_type in _str_to_list(account_types):
            try:
                plan = next((p for p in Config['SUBSCRIPTIONS'] if p['plan'] == account_type), None)
                if plan:
                    subscriptions.append(plan)
            except Exception: pass
    else:
        subscriptions = [sub for sub in Config['SUBSCRIPTIONS']]
    
    return GetSubscriptionPlansDTO(subscriptions).dto()

