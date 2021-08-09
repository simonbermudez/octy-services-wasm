from fastapi.responses import JSONResponse
from config import Config

### Versioning DTO
class VersioningDTO():
    def __init__(self, application_name : str, app : str, previous : str):
        self.application_name = application_name
        self.app = app
        self.previous = previous

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Versioning information found.'},
                    'application_name' : self.application_name,
                    'current_version' : Config[self.app],
                    'version_url' : Config['VERSION_URL'],
                    'previous_versions' : Config[self.previous]
            }
        )

