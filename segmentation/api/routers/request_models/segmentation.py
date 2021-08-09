from pydantic import BaseModel, validator
from typing import List, Optional, Any
from config import Config

### Create Segment Input Schema
class EventSequenceEvent(BaseModel):
    exp_timeframe : int
    action_inaction : str
    @validator('action_inaction')
    def allowed_status(cls, value, **kwargs):
        choices = ['action', 'inaction']
        if value not in choices:
            raise ValueError('Invalid event provided. Please ensure \'action_inaction\' parameter is either \'action\' or \'inaction\'')
        return value
    # event : str
    event_type : str
    event_properties : Optional[dict]
    @validator('event_properties')
    def allowed_value(cls, value, **kwargs):
        if value == {}:
            value = None
        if isinstance(value, dict) == False and value != None:
            raise ValueError('The \'events_properties\' parameter must contain a single key value pair object, or a null value.')
        return value



class CreateSegment(BaseModel):
    segment_name : str
    segment_type : str
    segment_sub_type : int
    segment_timeframe : int
    event_sequence : List[EventSequenceEvent]
    profile_property_name : Optional[str] = None
    profile_property_value : Optional[Any] = None
    @validator('profile_property_value')
    def allowed_types(cls, value, **kwargs):
        types = [int,str,bool,float ]
        if type(value) not in types:
            raise ValueError('The \'profile_property_value\' parameter must be of type: int or str or bool or float or null.')
        return value

class DeleteSegments(BaseModel):
    segments : List[str]
    @validator('segments')
    def length(cls, v):
        if len(v) > Config['MAX_DELETE_SEGMENTS']:
            raise ValueError('You can only delete up to 100 segments per request.')
        return v

