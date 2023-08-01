from fastapi.responses import JSONResponse

class DeleteAccountJobsDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Octy Jobs deleted.'},
                    'is_deleted' : self.is_deleted
            }
        )

