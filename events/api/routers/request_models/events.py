from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config


### Create event Input Schema
class CreateEvent(BaseModel):
    event_type : str
    event_properties : dict
    profile_id : str
    created_at : Optional[str] # only acknowledged if batch create

### Delete event types Input Schema
class BatchCreateEvents(BaseModel):
    events : List[CreateEvent]
    @validator('events')
    def length(cls, v):
        if len(v) > Config['MAX_CREATE_EVENTS']:
            raise ValueError(f'You can only create up to {Config["MAX_CREATE_EVENTS"]} events per request.')
        return v


class GetEventsInternal(BaseModel):
    timeframe : int
    account_id : str
    event_sequence_event : Optional[Dict]
    profile_ids : Optional[List[str]]
    event_type : Optional[str]


### Delete events for an account Input Schema
class DeleteEventsInternal(BaseModel):
    account_id : str
