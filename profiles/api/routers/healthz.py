from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse

router = APIRouter()

######################################
# Route : /healthz
# Request type : GET
# Required parameters : nil
# Description : K8 pod healthz check
# Returns : OK
# Limits : nil
# Requires auth : NO
######################################

@router.get('/healthz')
async def healthz(request: Request):
    return JSONResponse(
            status_code=200,
            content='OK'
        )


