#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.recommendation import *
from services.recommendation import RecommendationsService
from .dto.recommendation import *

#python imports
from typing import Optional, List

#external imports
from fastapi import APIRouter, Request, Query, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Recommendations API endpoints
######################################

######################################
# Route : /v1/retention/recommendations
# Request type : POST
# Required parameters : profile_ids list [strings]
# Description : Allow client to get latest cached item recomendations.
# Returns : Each item based recomendation per valid profile id provided
# Limits : 120 Requests per minute. Max 100 profile_ids
# Requires auth : YES -- Public Key & Secret Key
######################################
@router.post('/v1/retention/recommendations', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def get_recomendations(request: Request,  getRecomendations : GetRecomendations,
    current_account: Account = Depends(decode_account_jwt)):
    if len(getRecomendations.profile_ids) > Config['MAX_REC_PREDICTIONS']:
        raise OctyException(400,'Recommendation request limit exceeded.', 
            [{'error_message' : f'A maximum number of {Config["MAX_REC_PREDICTIONS"]} profile ids per recommendations request allowed.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])
    recommendations, training_job_meta = await RecommendationsService(account=current_account, account_id=None)\
        .get_recommendations(profile_ids=getRecomendations.profile_ids)
    return GetRecommendationsDTO(recommendations=recommendations, training_job_meta=training_job_meta).dto()


######################################
# Recommendations INTERNAL API endpoints
# Available via cluster IP only.
######################################

######################################
# Route : /v1/internal/recommendations
# Request type : POST
# Required parameters : profile_ids list [strings]
# Description : Allow client to get latest cached item recomendations.
# Returns : Each item based recomendation per valid profile id provided
# Limits : 120 Requests per minute. Max 100 profile_ids
# NOTE : Do not expose route in ingress
######################################
@router.post('/v1/internal/recommendations', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def get_recomendations(request: Request,  getRec : GetRecomendationsInternal):
    if len(getRec.profile_ids) > Config['MAX_REC_PREDICTIONS']:
        raise OctyException(400,'Recommendation request limit exceeded.', 
            [{'error_message' : f'A maximum number of {Config["MAX_REC_PREDICTIONS"]} profile ids per recommendations request allowed.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])
    recommendations, training_job_meta = await RecommendationsService(account=None, account_id=getRec.account_id)\
        .get_recommendations(profile_ids=getRec.profile_ids)
    return GetRecommendationsDTO(recommendations=recommendations, training_job_meta=training_job_meta).dto()

######################################
# Route : /v1/internal/recommendations/delete
# Request type : POST
# Required parameters : DeleteAccountRecommendations
# Description : Delete all recommendations for an account.
# Returns : Bool -- True if all recommendations were deleted successfully, False otherwise
# NOTE : Do not expose route in ingress
######################################
@router.post('/v1/internal/recommendations/delete')
async def get_recomendations(request: Request,  e : DeleteAccountRecommendations):
    res = await RecommendationsService(account=None, account_id=e.account_id).delete_account_recommendations()
    return DeleteAccountRecommendationsDTO(res).dto()