#module imports 
from .error_handlers import *

#python imports

#external imports


######################################
# Request validations
######################################

async def validate_pagination_request(request : Request):

    try:
        if type(int(request.headers['cursor'])) != int:
            return False, 'Pagination header (-H cursor: int) must be of type int'
    except Exception:
        return False, 'Please set a pagination header (-H cursor: int)'
    return request.headers['cursor'], None
