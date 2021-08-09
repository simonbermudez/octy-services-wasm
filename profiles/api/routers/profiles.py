#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.profiles import *
from .request_models.account import Account
from services.profiles import ProfilesService
from .dto.profiles import *

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
# Profile routers:
# Profile API endpoints
######################################

######################################
# Route : /v1/retention/profiles/<profile_id/ clients customer_id if reference> (optional)
# Request type : GET
# Required parameters : null
# Description : Access all customer profiles. Or all customer profiles associated with a specified profile_id
# Returns : customer_profile object(s)
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/profiles')
@limiter.limit("120/minute")
async def get_customer_profiles(request: Request, 
    id : Optional[str] = None, 
    rfm : Optional[str] = None, 
    churn_prob : Optional[str] = None, 
    segments : Optional[str] = None,
    current_account: Account = Depends(decode_account_jwt)):

    async def validate_arg_format(arg_):
        #rfm & ltv format : int-int
        if '-' not in arg_:
            return False,[]
        if arg_.count('-') > 1:
                return False,[]
        #split by - character
        scores=arg_.split('-')
        x=[]
        for score in scores:
            if score == '':
                continue
            try:
                int(score)
                x.append(
                    int(score)
                )
            except ValueError:
                return False,[]
        return True, x

    rfm_vals=None

    cursor = 0
    if id == None:

        if rfm != None:
            res,rfm_vals = await validate_arg_format(rfm)
            if res==False:
                raise OctyException(400,'Invalid query string argument', [{'message' : 'rfm argument provided in an invalid format. Required format : int-int', 
                    'extended_help': Config['PROFILES_EXTENDED_HELP']}])
        
        if churn_prob != None:
            try:
                int(churn_prob)
                raise OctyException(400,'Invalid query string argument', [{'message' : 'churn_prob argument provided in an invalid format. Required format : string (low, mid, high, very-high)', 
                    'extended_help': Config['PROFILES_EXTENDED_HELP']}])
            except ValueError:
                pass
        
        if segments != None:
            segments=segments.split(',')
            for s in segments:
                if s == '' or s == None:
                    segments.remove(s)

        # Validate pagination headers set
        cursor, pag_message = await validate_pagination_request(request,id)
        if cursor == None:
            raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
                'extended_help': Config['PROFILES_EXTENDED_HELP']}])
    
    profiles, total = ProfilesService(current_account).get_profiles(segments=segments,
                                                            rfm_values=rfm_vals, 
                                                            churn_prob=churn_prob,
                                                            id_=id,
                                                            cursor=cursor)
    return GetProfilesDTO(profiles, total, cursor).dto()

######################################
# Route : /v1/retention/profiles/create
# Request type : POST
# Required parameters : profiles list (customer_id [string], profile_data [string], platform_info [string])
# Description : Allow client to create new customer profiles.
# Returns : Status of customer profile creation, each profile_id
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/profiles/create',
            dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def create_customer_profiles(request: Request, profiles : CreateProfiles,
    current_account: Account = Depends(decode_account_jwt)):
    created, failed = ProfilesService(current_account).create_profiles(profiles)
    return CreateProfilesDTO(created, failed).dto()


######################################
# Route : /v1/retention/profiles/update
# Request type : POST
# Required parameters : profiles list (profile_id [string], customer_id [string], profile_data [string], platform_info [string])
# Description : Allow client to update existing customer profiles.
# Returns : List of updated customer profiles / not found profiles
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/profiles/update',
            dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def update_customer_profiles(request: Request, update_profiles : UpdateProfiles,
    current_account: Account = Depends(decode_account_jwt)):
    updated, failed = await ProfilesService(current_account).update_profiles(update_profiles, internal=False)
    return UpdateProfilesDTO(updated, failed).dto()


######################################
# Route : /v1/retention/profiles/delete
# Request type : POST
# Required parameters : profiles list (profile_id [string])
# Description : Allow client to delete existing customer profiles.
# Returns : Status of customer profiles deletion
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/profiles/delete', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def delete_customer_profiles(request: Request,  delete_profiles : DeleteProfiles,
    current_account: Account = Depends(decode_account_jwt)):
    delete_profiles, failed = await ProfilesService(current_account).delete_profiles(delete_profiles)
    return DeleteProfilesDTO(delete_profiles, failed).dto()


######################################
# Internal Profiles API endpoints. 
# Available via cluster IP only.
######################################

######################################
# Route : /v1/internal/profiles
# Request type : POST
# Required parameters : query-string {ids return ids only #?ids=true}
# POST body : {"profiles" : ["profile_id-123", ...]}
# Description : Internal service used to get profiles
# Returns : Profiles, not found profile ids
# NOTE : Do not expose route in ingress
######################################

@router.post('/v1/internal/profiles') 
async def get_profiles_internal(request: Request,  profiles : GetProfilesInternal, ids : bool, status : str = 'active'):

    cursor = 0
    if profiles.get_all:
    # Validate pagination headers set
        cursor, pag_message = await validate_pagination_request(request, None)
        if cursor == None:
            raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
                'extended_help': ''}])

    else:
        # do not allow more than 2000 profile ids
        if len(profiles.profiles) > 2000:
            raise OctyException(400,'Exceeded resource request limit', [{'message' : 'can only get 2000 profiles per request', 
                'extended_help': ''}])


    profiles, not_found , total = ProfilesService(None, profiles.account_id).get_profiles_internal(profiles=profiles,status = status,  cursor=cursor, ids=bool(ids))
    return GetProfilesInternalDTO(profiles, not_found , total, cursor).dto()

'''
Scenarios:

- creating events (send IDS, need valid ids and invalid ids returned. No pagination or cursor)
- Training data (get all paginated - full profiles)
- Rec predictions (send IDS, need valid ids and invalid ids returned. No pagination or cursor)
- Messages (when rec embedded) (send IDS, need valid ids and invalid ids returned. No pagination or cursor)
'''