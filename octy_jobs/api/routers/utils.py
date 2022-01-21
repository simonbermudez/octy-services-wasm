#module imports 
from .error_handlers import *
from config import Config

#python imports
from functools import wraps
import re
import json

#external imports


######################################
# Request validations (Consumed via Dependency Injection)
######################################

async def validate_post_headers(request : Request) -> None:
    
    try:
        if request.headers['content-type'] != 'application/json':
            raise OctyException(400,'Missing header',[{'error_message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['ERRORS_OVERVIEW_EXTENDED_HELP']}])
    except KeyError:
        raise OctyException(400,'Missing header',[{'error_message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['ERRORS_OVERVIEW_EXTENDED_HELP']}])
        
        
    try:
        if request.headers['content-length'] == None or request.headers['content-length'] == '':
            raise OctyException(411,'Invalid headers provided', [{'error_message' : '[Content-Length] header must be provided in request headers.', 'extended_help': ''}])
    except KeyError:
        raise OctyException(411,'Invalid headers provided', [{'error_message' : '[Content-Length] header must be provided in request headers.', 'extended_help': ''}])

    try:
        request.headers['http-transfer-encoding']
        raise OctyException(501,'Invalid headers provided', [
            {
                'error_message' : '[Transfer-Encoding] header must NOT be provided in request headers as it is not supported.', 
                'extended_help': ''
            }
        ])
    except KeyError:
        pass
