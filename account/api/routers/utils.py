#module imports 
from .error_handlers import *
from config import Config

#python imports


#external imports



######################################
# Routers utils functions
######################################

async def validate_post_headers(request : Request) -> None:
    """
        A function used to validate the headers of any POST request

        Parameters
        ----------
        request : Starlette Request instance

        Returns
        ----------
        None
    """
    
    try:
        if request.headers['content-type'] != 'application/json':
            raise OctyException(400,'Missing header',[{'message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['INVALID_JSON_EXTENDED_HELP']}])
    except KeyError:
        raise OctyException(400,'Missing header',[{'message' : '[Content-Type] : [application/json] header must be provided in request headers.', 
            'extended_help': Config['INVALID_JSON_EXTENDED_HELP']}])
        
        
    try:
        if request.headers['content-length'] == None or request.headers['content-length'] == '':
            raise OctyException(411,'Invalid headers provided', [{'message' : '[Content-Length] header must be provided in request headers.', 'extended_help': ''}])
    except KeyError:
        raise OctyException(411,'Invalid headers provided', [{'message' : '[Content-Length] header must be provided in request headers.', 'extended_help': ''}])

    try:
        request.headers['http-transfer-encoding']
        raise OctyException(501,'Invalid headers provided', [
            {
                'message' : '[Transfer-Encoding] header must NOT be provided in request headers as it is not supported.', 
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
                return None, "Pagination header (-H cursor: int) must be of type int"
        except Exception:
            return None, "Please provide a valid object identifier within the query string eg: (?id=) or set a pagination header (-H cursor: int)"
        return int(request.headers['cursor']), None
    return None, None