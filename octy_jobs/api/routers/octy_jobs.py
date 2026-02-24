#module imports 
from .error_handlers import *
from .utils import *
from .request_models.octy_jobs import *
from services.octy_jobs import OctyJobQueueService

#python imports

#external imports
from fastapi import APIRouter, Request
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# octy_jobs routers:
# octy_jobs API endpoints 
######################################

######################################
# Route : /v1/internal/octy-jobs
# Request type : POST
# Required parameters : {account_id (mandatory), job_ids (optional)}
# Description : Gets running octy-jobs. all or specified 
# Returns : octy-jobs : list
######################################


######################################
# Route : /v1/internal/jobs/callback
# Request type : POST
# Required parameters : OctyJobCallBack request model
# Description : Updates the status of an Octy job.
# Returns : OK -- 200
######################################
@router.post('/v1/internal/jobs/callback')
async def octy_job_callback(request: Request, cb: OctyJobCallBack):
    await OctyJobQueueService(cb.account_id).status_callback(cb.dict())
    return "OK"

######################################
# Route : /v1/internal/jobs/delete
# Request type : POST
# Required parameters : DeleteAccountJobs request model
# Description : Updates the status of an Octy job.
# Returns : bool -- True if all jobs were deleted successfully, False otherwise
######################################
@router.post('/v1/internal/jobs/delete')
async def delete_account_jobs(request: Request, e: DeleteAccountJobs):
    res = await OctyJobQueueService(e.account_id).delete_all_octy_jobs()
    return DeleteAccountJobsDTO(res).dto()