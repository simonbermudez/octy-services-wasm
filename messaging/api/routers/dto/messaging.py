from fastapi.responses import JSONResponse


### Get Templates DTO
class GetTemplatesDTO():
    def __init__(self, templates, total, cursor):
        self.templates = templates
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Messaging templates found.', 'count' : len(self.templates), 'total' : self.total},
                    'templates' : self.templates
            },
            headers={'cursor' : str(self.cursor+len(self.templates))}
        )

### Create Templates DTO
class CreateTemplatesDTO():
    def __init__(self, templates, failed_to_create):
        self.templates = templates
        self.failed_to_create = failed_to_create

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Successfully created new message templates.'},
                    'templates' : self.templates,
                    'failed_to_create' : self.failed_to_create
            }
        )

### Update Templates DTO
class UpdateTemplatesDTO():
    def __init__(self, templates, failed_to_update):
        self.templates = templates
        self.failed_to_update = failed_to_update

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Message templates updated.'},
                    'templates' : self.templates,
                    'failed_to_update' : self.failed_to_update
            }
        )

### Delete Templates DTO
class DeleteTemplatesDTO():
    def __init__(self, deleted_templates, failed_to_delete):
        self.deleted_templates = deleted_templates
        self.failed_to_delete = failed_to_delete

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Message templates deleted.'},
                    'deleted_templates' : self.deleted_templates,
                    'failed_to_delete' : self.failed_to_delete
            }
        )

### Generate Content DTO
class GenerateContentDTO():
    def __init__(self, created_messages, failed_messages, failed_templates):
        self.created_messages = created_messages
        self.failed_messages = failed_messages
        self.failed_templates = failed_templates

    def dto(self) -> JSONResponse:
        msg='Successfully generated content'
        if len(self.created_messages)<1:
            msg='No content generated'
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : msg, 'count' : len(self.created_messages)},
                    'generated_messages' : self.created_messages,
                    'failed_messages' : self.failed_messages,
                    'failed_templates' : self.failed_templates
            }
        )