# module imports
from .utils import *
from services.auth import authService
from .dto.auth import *

# python imports

# external imports
from fastapi import APIRouter, Request, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address

router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Auth routers:
# Authentication / Authorization endpoints
######################################

######################################
# Route : /v1/account/authenticate
# Request type : GET
# Required parameters : null
# Description : Create new Octy account
# Returns : Auth status, Authorization/ account info fat jwt token
# Limits : 600 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/account/authenticate',
            dependencies=[Depends(authService.authenticate_account)])
@limiter.limit("600/minute")
async def authenticate_account(request: Request):
    return AuthenticateDTO(await authService.get_auth_token(request)).dto()
