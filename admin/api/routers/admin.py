#module imports 
from .error_handlers import *
from .dto.admin import *
from config import Config


#python imports
from typing import Optional
import json

#external imports
from fastapi import APIRouter, Request, Depends
from fastapi.responses import JSONResponse
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Admin application routers:
# Admin  application API endpoints. 
# Endpoints that will be consumed by trusted application
######################################


######################################
# Route : /v1/admin/application/versioning
# Request type : GET
# Required parameters : app : str -- Name of application you require version info for.
# Description : Return the current version information for the specified octy application.
# Returns : version info
# Limits : 600 requests per minute
# Requires auth : YES -- (Trusted App auth) - -u client_id -p client_secret
######################################

@router.get('/v1/admin/application/versioning')
@limiter.limit("600/minute")
async def version_info(request : Request, app : Optional[str] = None):

    if app == 'api':
        return VersioningDTO(app, "CURRENT_API_VERSION",
                             "PREVIOUS_API_VERSIONS").dto()

    elif app == 'cli':
        return VersioningDTO(app, "CURRENT_CLI_VERSION",
                             "PREVIOUS_CLI_VERSIONS").dto()

    else:
        raise OctyException(400, 'Invalid query string argument',
                            [{'message': 'invalid \'app\' query parameter provided. Accepted values: \'api\' or \'cli\'',
                              'extended_help': ''}])



######################################
# Route : /v1/admin/application/resources/format
# Request type : GET
# Required parameters : type : str -- resource type
# Description : Return the resource format of specified resource.
# Returns : required resource format [object]
# Limits : 600 requests per minute
# Requires auth : YES -- (Trusted App auth) - -u client_id -p client_secret
######################################

@router.get('/v1/admin/application/resources/format')
@limiter.limit("600/minute")
async def version_info(request : Request, type : Optional[str] = None):

    if type not in ['events', 'items', 'profiles']:
        raise OctyException(400, 'Invalid query string argument',
                            [{'message': 'invalid \'type\' query parameter provided. Accepted values: \'events\' or \'items\' or \'profiles\'',
                            'extended_help': ''}])

    print(Config['RESOURCE_FORMAT_EXAMPLES_DIR'] + type + ".json")

    with open(Config['RESOURCE_FORMAT_EXAMPLES_DIR'] + type + ".json") as file:
        resource_format = json.loads(file.read())


    return JSONResponse(
        status_code=200,
        content=resource_format
    )