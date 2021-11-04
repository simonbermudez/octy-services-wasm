from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config

### Create event types Input Schema
class CreateEventTypesChild(BaseModel):
    event_type : str
    event_properties : List[str]

class CreateEventTypes(BaseModel):
    event_types : List[CreateEventTypesChild]
    @validator('event_types')
    def length(cls, v):
        if len(v) > Config['MAX_CREATE_EVENT_TYPES']:
            raise ValueError(f'You can only create up to {Config["MAX_CREATE_EVENT_TYPES"]} custom event types per request.')
        return v


### Delete event types Input Schema
class DeleteEventTypes(BaseModel):
    event_type_ids : List[str]
    @validator('event_type_ids')
    def length(cls, v):
        if len(v) > Config['MAX_DELETE_EVENT_TYPES']:
            raise ValueError(f'You can only delete up to {Config["MAX_DELETE_EVENT_TYPES"]} custom event types per request.')
        return v

### Get event types internal Input Schema
class GetEventTypesInternal(BaseModel):
    account_id : str
    event_type_names : List[str]