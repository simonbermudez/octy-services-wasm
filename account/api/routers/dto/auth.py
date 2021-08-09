from fastapi.responses import JSONResponse

### Authentication DTO
class AuthenticateDTO():
    def __init__(self, auth_token : str):
        self.auth_token = auth_token

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Successfully generated account authorization token'},
                'auth' : {
                    'jwt_token' : self.auth_token
                }
            },
            headers={
                'X-AUTH-JWT-TOKEN' : self.auth_token
            }
        )