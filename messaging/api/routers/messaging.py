#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.messaging import *
from .dto.messaging import *
from services.messaging import MessagingService
from services.template_engine import TemplateEngine

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
# Messaging routers:
# Messaging API endpoints
######################################

######################################
# Route : /v1/retention/messaging/templates
# Request type : GET
# Required parameters : null
# Description : Access all created templates. Or a specific template based on template_id or friendly_name or template_type.
# Returns : Data and content on each found template
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/messaging/templates')
@limiter.limit("120/minute")
async def get_templates(request: Request, 
    ids : Optional[str] = None,
    current_account: Account = Depends(decode_account_jwt)):
    
    identifiers=None
    cursor = 0

    def remove_first_end_spaces(string):
        return "".join(string.rstrip().lstrip())

    if ids == None:
        # Validate pagination headers set
        cursor, pag_message = await validate_pagination_request(request,ids)
        if cursor == None:
            raise OctyException(400,'Missing Parameters', [{'error_message' : pag_message, 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
    else:
        identifiers = ids.split(",")
        identifiers = list(dict.fromkeys(filter(None, identifiers)))
        identifiers = [remove_first_end_spaces(i) for i in identifiers]

        if len(identifiers) > Config['MAX_GET_TEMPLATES']:
            raise OctyException(400,'Invalid Parameters', [{'error_message' : f'A maximum number of {Config["MAX_GET_TEMPLATES"]} identifiers can be provided with the "?ids=" query param per request', 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
    
    templates, total = await MessagingService(account=current_account).get_templates(identifiers=identifiers, cursor=cursor)
    return GetTemplatesDTO(templates, total, cursor).dto()


######################################
# Route : /v1/retention/messaging/templates/create
# Request type : POST
# Required parameters : friendly_name [string], template_type [string], title [string],content [string] ,required_data list[string], default_values []
# Description : Allow client to create new NLG templates.
# Returns : Status of template creation, each template_id and friendly
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/messaging/templates/create', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def create_templates(request: Request, 
    templates : CreateTemplates,
    current_account: Account = Depends(decode_account_jwt)):
    templates, failed_to_create = await MessagingService(account=current_account).create_templates(templates=templates)
    return CreateTemplatesDTO(templates, failed_to_create).dto()


######################################
# Route : /v1/retention/messaging/templates/update
# Request type : POST
# Required parameters : friendly_name [string], template_type [string], title [string],content [string] ,required_data list[string], default_values [] & template_id of each template to update
# Description : Allow client to update existing NLG templates.
# Returns : Status of template updation, each template_id and friendly
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/messaging/templates/update', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def update_templates(request: Request, 
    update_templates : UpdateTemplates,
    current_account: Account = Depends(decode_account_jwt)):
    templates, failed_to_create = await MessagingService(account=current_account).update_templates(templates=update_templates)
    return UpdateTemplatesDTO(templates, failed_to_create).dto()


######################################
# Route : /v1/retention/messaging/templates/delete
# Request type : POST
# Required parameters : template_ids
# Description : Allow client to delete existing NLG templates.
# Returns : Status of template deletion, each template_id 
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/messaging/templates/delete', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def delete_templates(request: Request, 
    delete_templates : DeleteTemplates,
    current_account: Account = Depends(decode_account_jwt)):
    deleted_templates, failed_to_delete = await MessagingService(account=current_account).delete_templates(templates=delete_templates)
    return DeleteTemplatesDTO(deleted_templates, failed_to_delete).dto()


######################################
# Route : /v1/retention/messaging/content/generate
# Request type : POST
# Required parameters : template_id [string], friendly_name [string], item_recommendation [bool], data [input data]
# Description : Allow client to generate content using NLG templates.
# Returns : Status of content creation, each generated message
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/messaging/content/generate', 
    dependencies=[Depends(validate_post_headers)])
@limiter.limit("120/minute")
async def generate_content(request: Request, 
    messages : GenerateContent,
    current_account: Account = Depends(decode_account_jwt)):
    t = TemplateEngine(account=current_account)
    await t.generate(messages=messages)
    return GenerateContentDTO(t.created_messages, t.failed_messages, t.failed_templates).dto()