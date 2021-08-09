from fastapi.responses import JSONResponse
import json

### Get Items DTO
class GetItemsDTO():
    def __init__(self, items, total, cursor):
        self.items = items
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Items found.',
                                 'count' : len(self.items), 'total' : self.total},
                'items' : self.items
            },
            headers={'cursor' : str(self.cursor+len(self.items))}
        )

### Create Items DTO
class CreateItemsDTO():
    def __init__(self, created_items, failed_to_create):
        self.created_items = created_items
        self.failed_to_create = failed_to_create

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Items created.',
                                 'count' : len(self.created_items)},
                'items' : self.created_items,
                'failed_to_create' : self.failed_to_create
            }
        )


### Update Items DTO
class UpdateItemsDTO():
    def __init__(self, updated_items, failed_to_update):
        self.updated_items = updated_items
        self.failed_to_update = failed_to_update

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Items updated.'},
                'items' : self.updated_items,
                'failed_to_update' : self.failed_to_update
            }
        )


### Delete Items DTO
class DeleteItemsDTO():
    def __init__(self, deleted_items, failed_to_delete):
        self.deleted_items = deleted_items
        self.failed_to_delete = failed_to_delete

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Items deleted.'},
                'deleted_items' : self.deleted_items,
                'failed_to_delete' : self.failed_to_delete
            }
        )
