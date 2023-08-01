from pydantic import BaseModel
from fastapi.responses import JSONResponse
from typing import List, Dict


### Octyjob Callback Input Schema
class OctyJobCallBack(BaseModel):
    account_id : str
    octy_job_id : str
    message : str
    status : str

class DeleteAccountJobs(BaseModel):
    account_id: str

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

