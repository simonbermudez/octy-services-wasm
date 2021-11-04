#module imports 
from .error_handlers import *
from config import Config
from .utils import *
from .request_models.segmentation import *
from .dto.segmentation import *
from services.segmentation import SegmentationService

#python imports
from typing import Optional
import re

#external imports
from fastapi import APIRouter, Request, Depends
from slowapi import Limiter
from slowapi.util import get_remote_address


router = APIRouter()
limiter = Limiter(key_func=get_remote_address)


######################################
# Segmentation routers:
# Segmentation API endpoints
######################################


######################################
# Route : /v1/retention/segments/?ids=<segment_id(s) | segment_name(s)>,... (optional - max 100)
# Request type : GET
# Required parameters : null
# Description : Access all created segments summaries. Or all segments associated with a specified segment_id or segment_name
# Returns : Summary of each created segment
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.get('/v1/retention/segments')
@limiter.limit("120/minute")
async def get_segments(request: Request, 
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
            raise OctyException(400,'Missing Parameters', [{'message' : pag_message, 
                'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
    else:
        identifiers = ids.split(",")
        identifiers = list(dict.fromkeys(filter(None, identifiers)))
        identifiers = [remove_first_end_spaces(i) for i in identifiers]

        if len(identifiers) > Config['MAX_GET_SEGMENTS']:
            raise OctyException(400,'Invalid Parameters', [{'message' : f'A maximum number of {Config["MAX_GET_SEGMENTS"]} identifiers can be provided with the "?ids=" query param per request', 
                'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
    
    segments, total = SegmentationService(current_account).get_segments(identifiers=identifiers,cursor=cursor)

    return GetSegmentsDTO(segments, total, cursor).dto()


######################################
# Route : /v1/retention/segments/create
# Request type : POST
# Required parameters : segment_name [string], segment_type [string], segment_sub_type [int], segment_timeframe [int],event_sequence list(event_type [string], event_properties [raw], time_interval [int])
# Description : Create segment
# Returns : Summary and status of created segment
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/segments/create')
@limiter.limit("120/minute")
async def create_segments(request: Request, 
    segment : CreateSegment,
    current_account: Account = Depends(decode_account_jwt)):
    created_segment, message =  await SegmentationService(current_account).create_segment(segment)
    return CreateSegmentDTO(created_segment, message).dto()


######################################
# Route : /v1/retention/segments/delete
# Request type : POST
# Required parameters : segment_id [string]
# Description : Delete segments 
# Returns : Id and status of deleted or un found segment
# Limits : 120 Requests per minute
# Requires auth : YES -- Public Key & Secret Key
######################################

@router.post('/v1/retention/segments/delete')
@limiter.limit("120/minute")
async def delete_segments(request: Request, 
    segment_ids : DeleteSegments,
    current_account: Account = Depends(decode_account_jwt)):
    deleted_segments, failed_to_delete = await SegmentationService(current_account).delete_segments(segment_ids.segments)
    return DeleteSegmentsDTO(deleted_segments, failed_to_delete).dto()



######################################
# Internal Segmentation API endpoints
######################################


######################################
# Route : /v1/internal/segments
# Request type : GET
# Required parameters : account_id : str, segment_type : str, status : str
# Description : Access all created segments summaries.
# Returns : Summary of each created segment
######################################

@router.get('/v1/internal/segments')
async def get_segments_internal(account_id : str, segment_type : str, status : str):
    cursor=0
    segments, total = SegmentationService(None,account_id=account_id).get_segments(cursor=cursor, status=status, segment_type=segment_type, internal=True)
    return GetSegmentsDTO(segments, total, cursor).dto()
