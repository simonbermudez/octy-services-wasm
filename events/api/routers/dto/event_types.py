from fastapi.responses import JSONResponse


### Get EventTypes DTO
class GetEventTypesDTO():
    def __init__(self, event_types, total, cursor):
        self.event_types = event_types
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Custom event type(s) found.', 'count' : len(self.event_types), 'total' : self.total},
                    'event_types' : self.event_types
            },
            headers={'cursor' : str(self.cursor+len(self.event_types))}
        )

### Create EventTypes DTO
class CreateEventTypesDTO():
    def __init__(self, event_types, failed_to_create):
        self.event_types = event_types
        self.failed_to_create = failed_to_create

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Custom event type(s) created.'},
                    'event_types' : self.event_types,
                    'failed_to_create' : self.failed_to_create
            }
        )

### Delete EventTypes DTO
class DeleteEventTypesDTO():
    def __init__(self, deleted_event_types, failed_to_delete):
        self.deleted_event_types = deleted_event_types
        self.failed_to_delete = failed_to_delete

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Custom event type(s) deleted.'},
                    'deleted_event_types' : self.deleted_event_types,
                    'failed_to_delete' : self.failed_to_delete
            }
        )


### Get EventTypes Internal DTO
class GetEventTypesInternalDTO():
    def __init__(self, event_types, not_found):
        self.event_types = event_types
        self.not_found = not_found

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Custom event type(s) found.'},
                    'event_types' : self.event_types,
                    'not_found' : self.not_found
            }
        )