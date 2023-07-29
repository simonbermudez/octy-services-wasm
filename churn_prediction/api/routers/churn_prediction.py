#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from services.churn_prediction import ChurnPredictionService
from .dto.churn_prediction import *
from .request_models.churn_prediction import *

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
# Churn prediction API endpoints
######################################


######################################
# Route : /v1/retention/churn_prediction/report
# Request type : GET
# Required parameters : null
# Description : Generate and return a churn prediction report indicating features of importance, churn calculations and training_job meta data
# Returns : Churn report
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/churn_prediction/report')
@limiter.limit("120/minute")
async def get_churn_report(request: Request, current_account: Account = Depends(decode_account_jwt)):
    churn_report = await ChurnPredictionService(account=current_account).generate_churn_report()
    return GenerateChurnReportDTO(churn_prediction_report=churn_report).dto()

######################################
# Route : /v1/internal/churn_prediction/delete
# Request type : POST
# Required parameters : DeleteAccountChurnPredictions
# Description : Delete all churn predictions data associated with an account
# Returns : Bool indicating success or failure
######################################

@router.post('/v1/internal/churn_prediction/delete')
async def delete_churn_prediction_internal(e : DeleteAccountChurnPredictions):
    res = await ChurnPredictionService(None,account_id=e.account_id).delete_account_churn_predictions_internal()
    return DeleteAccountChurnPredictionsDTO(res).dto()