from fastapi.responses import JSONResponse
from pydantic.json import pydantic_encoder
import json
from datetime import datetime as dt

### Create Event DTO
class CreateEventDTO():
    def __init__(self, event):
        self.event = event

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Event created.'},
                    'event_id' : self.event['event_id'],
                    'event_type' : self.event['event_type'],
                    'event_properties' : self.event['event_properties'],
                    'profile_id' : self.event['profile_id'],
                    'created_at' : dt.now().strftime('%a, %d %b %Y %H:%M:%S GMT')
            }
        )

### Batch Create Events DTO
class BatchCreateEventsDTO():
    def __init__(self, valid_events, invalid_events):
        self.valid_events = valid_events
        self.invalid_events = invalid_events

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Events created.', 'count' : len(self.valid_events)},
                    'created_events' : self.valid_events,
                    'failed_to_create' : self.invalid_events
            }
        )


### Internal get Events DTO
class InternalGetEventsDTO():
    def __init__(self, events, total):
        self.events = events
        self.total = total

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Events found.', 'count' : len(self.events), 'total' : self.total},
                    'events' : self.events
            }
        )
    

### Internal get Events DTO
class InternalGetEventDTO():
    def __init__(self, event):
        self.event = event

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Event found.'},
                    'event' : self.event
            }
        )
    
    
### Delete Events DTO
class InternalDeleteEventsDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'All Events associated with account deleted.'},
            }
        )
