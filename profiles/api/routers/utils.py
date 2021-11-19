#module imports 
from .error_handlers import *
from config import Config
from .request_models.account import Account
from utils.utils import *

#python imports
from functools import wraps
from datetime import datetime as dt
from datetime import timezone as tz


#external imports
import jwt


######################################
# Auth (Consumed via Dependency Injection)
######################################

def decode_account_jwt(request : Request):
    '''
        Decode the auth JWT containing account information
        in the X-AUTH-JWT request header.
    '''
    try:
        request.headers['X-AUTH-JWT']
    except KeyError:
        raise OctyException(400,'Missing header',[{'error_message' : '[X-AUTH-JWT] : auth-token header must be provided in request headers.', 
            'extended_help': Config['INVALID_JSON_EXTENDED_HELP']}])

    with open('keys/octy-public-key.pub', 'rb') as f:
        public_key = f.read()

    try:
        decoded_token = jwt.decode(request.headers['X-AUTH-JWT'], public_key, algorithms='RS256')
        if decoded_token['m']['exp'] < dt_to_int(dt.now(tz.utc)):
            raise Exception(500, 'Authentication failed becuase of a server error. Invalid JWT token provided!')
    except jwt.InvalidTokenError as e:
        print(e)
        raise Exception(500, 'Authentication failed becuase of a server error. Invalid JWT token provided!')

    return Account(
        account_id = decoded_token['b']['a_id'],
        account_name = decoded_token['b']['a_n'],
        bucket = decoded_token['b']['b'],
        permissions = decoded_token['b']['pe'],
        account_configurations = decoded_token['b']['a_cf'],
        algorithm_configurations = decoded_token['b']['al_cf'],
        churn_info = decoded_token['b']['c_i'],
        created_at = decoded_token['b']['c_at']
    )


######################################
# Request validations (Consumed via Dependency Injection)
######################################

async def validate_post_headers(request : Request) -> None:
    
    try:
        if request.headers['content-type'] != "application/json":
            raise OctyException(400,'Missing header',[{'error_message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['INVALID_JSON_EXTENDED_HELP']}])
    except KeyError:
        raise OctyException(400,'Missing header',[{'error_message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['INVALID_JSON_EXTENDED_HELP']}])
        
        
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


async def validate_pagination_request(request : Request, identifier):
    # If query string argument supplied, a pagination cursor is not required.
    if identifier == None:
        try:
            if type(int(request.headers['cursor'])) != int:
                return None, "The value provided for the pagination header (-H cursor: str) could not be casted to type int."
        except Exception:
            return None, "Please provide a valid object identifier within the query string eg: (?id=) or set a pagination header (-H cursor: str)"
        return int(request.headers['cursor']), None
    return None, None