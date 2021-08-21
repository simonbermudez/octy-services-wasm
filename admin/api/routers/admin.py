#module imports 
from .error_handlers import *
from .dto.admin import *
from config import Config
from secrets import Secrets
from data.repositories.implementation.versioning_repository import versioningRepository


#python imports
from typing import Optional
import json
import urllib.parse
import hmac
import hashlib
import operator
import logging 

#external imports
from fastapi import APIRouter, Request, Depends
from fastapi.responses import JSONResponse
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)
logger = logging.getLogger('uvicorn')


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
@limiter.limit("60000/minute")
async def version_info(request : Request, app : Optional[str] = None):

    if app == 'api':
        versions = await versioningRepository.get_cached_version_data('octy-services')
        return VersioningDTO('Octy API', versions).dto()

    elif app == 'cli':
        versions = await versioningRepository.get_cached_version_data('octy-cli')
        return VersioningDTO('Octy CLI', versions).dto()

    else:
        raise OctyException(400, 'Invalid query string argument',
                            [{'message': 'invalid \'app\' query parameter provided. Accepted values: \'api\' or \'cli\'',
                              'extended_help': ''}])


######################################
# Route : /v1/admin/application/versioning/hook
# Request type : POST
# Description : Webhook to capture version information when a new release is made.
# Requires auth : YES -- 'GITHUB_WH_SECRET'
######################################

@router.post('/v1/admin/application/versioning/hook')
async def version_info_hook(request : Request):

    if not "X-Hub-Signature" in request.headers:
        raise OctyException(400, 'Invalid headers provided',
            [{'message': '', 'extended_help': ''}])
    
    wh_payload_bytes = await request.body()
    signature = request.headers['X-Hub-Signature']
    secret = Secrets['GITHUB_WH_SECRET'].encode()
    # contruct hmac generator with our secret as key, and SHA-1 as the hashing function
    hmac_gen = hmac.new(secret, wh_payload_bytes, hashlib.sha1)
    # create the hex digest and append prefix to match the GitHub request format
    digest = "sha1=" + hmac_gen.hexdigest()

    if not hmac.compare_digest(digest, signature):
        raise OctyException(401,'Authentication failed', [{'message' : 'Invalid hook secret provided with this request.', 
            'extended_help': ''}])

    wh_payload = json.loads(urllib.parse.parse_qs(wh_payload_bytes.decode('utf8').replace("'", '"'))['payload'][0])

    # If cli, Only cache release once all required assets have been publshed agasnt this release
    if 'cli' in wh_payload['repository']['name']:
        try:
            if wh_payload['action'] != 'edited':
                return 200
        except KeyError:
            pass
    else:
        # If other than a cli repo, only cache release on 'created' or 'edited' event.
        if wh_payload['action'] not in ["created", "edited"]:
            return 200

    # Determine release event repo
    if wh_payload['repository']['name'] in Config['REPOSITORIES']:

        logger.info(f'Caching release for repository {wh_payload["repository"]["name"]}')
        await versioningRepository.cache_version_data(
            data=wh_payload['release'], 
            repository_name=wh_payload['repository']['name']
        )

    else:
        raise OctyException(400, 'Invalid repo name provided',
                            [{'message': 'Not interested in releases on this repository',
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