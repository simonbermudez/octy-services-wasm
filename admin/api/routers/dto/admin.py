from fastapi.responses import JSONResponse
from config import Config

### Versioning DTO
class VersioningDTO():
    def __init__(self, application_type : str, versions : list):
        self.application_type = application_type
        [v.update({"change_log" : "*REDACTED*", "release_id" : "*REDACTED*"}) for v in versions]
        self.versions = versions

    def dto(self) -> JSONResponse:
        current_version = self.versions[0]
        previous_versions = [x for x in self.versions if not (current_version['id'] == x.get('id'))]
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Versioning information found.'},
                    'application_type'  :self.application_type,
                    'current_version' : current_version,
                    'previous_versions' : previous_versions
            }
        )

