from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config

### Create event types Input Schema
class CreateEventTypesChild(BaseModel):
    event_type : str
    @validator('event_type')
    def validate_event_type(cls, value, **kwargs):
        # length
        if len(value) > 60 or len(value) < 1:
            raise ValueError('Event types must be at least 1 character long and less than 60 characters long.')
        # allowed characters
        disallowed_characters = [',', '"', "'", ".", " "]
        found_characters = [c for c in disallowed_characters if c in value]
        if len(found_characters) > 0:
            raise ValueError(f'Illegal character(s) found in provided event type : {found_characters}')
        return value
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