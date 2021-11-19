# module imports
from config import Config

# python imports
from copy import deepcopy

# external imports
from fastapi import FastAPI, Request, status, HTTPException
from fastapi.exceptions import RequestValidationError
from fastapi.encoders import jsonable_encoder
from fastapi.responses import JSONResponse

# :: OCTY HTTP ERRORS (Exceptions with OctyException attributes)::
status_code_detail_map = {
    400: 'Bad request',
    401: 'Unauthorized',
    403: 'Forbidden',
    404: 'Resource not found',
    411: 'Length Required',
    415: 'Unsupported media type',
    501: 'Not Implemented'
}


class OctyException(Exception):
    def __init__(self, code: int, error_description: str, reasons: list):
        self.code = code
        self.error_description = error_description
        self.reasons = reasons


def add_exception_handlers(app: FastAPI) -> None:
    # Default HTTP Exceptions
    @app.exception_handler(404)
    async def not_found_error_handler(request: Request, exc: HTTPException):
        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = exc.detail
        err_template['error']['code'] = exc.status_code
        err_template['error'][
            'reason'] = 'The requested URL was not found on the server. \
If you entered the URL manually please check your spelling and try again.'

        return JSONResponse(
            status_code=exc.status_code,
            content=err_template,
        )

    @app.exception_handler(405)
    async def mna_error_handler(request: Request, exc: HTTPException):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = exc.detail
        err_template['error']['code'] = exc.status_code
        err_template['error']['reason'] = 'The method is not allowed for the requested URL'

        return JSONResponse(
            status_code=exc.status_code,
            content=err_template,
        )

    @app.exception_handler(413)
    async def entity_too_large_error_handler(request: Request, exc: HTTPException):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = exc.detail
        err_template['error']['code'] = exc.status_code
        err_template['error']['reason'] = 'The data supplied with this request exceeds the allowed size limit'

        return JSONResponse(
            status_code=exc.status_code,
            content=err_template,
        )

    @app.exception_handler(RequestValidationError)
    async def validation_handler(request: Request, exc: RequestValidationError):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['error']['errors'].clear()
        err_template['request_meta']['message'] = 'Unprocessable Entity'
        err_template['error']['code'] = status.HTTP_422_UNPROCESSABLE_ENTITY
        err_template['error']['reason'] = 'Missing or invalid JSON parameters'

        for error in exc.errors():
            err_template['error']['errors'].append(error)

        return JSONResponse(
            status_code=status.HTTP_422_UNPROCESSABLE_ENTITY,
            content=err_template,
        )

    @app.exception_handler(429)
    async def rate_limit_error_handler(request: Request, exc: HTTPException):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = 'Too many requests'
        err_template['error']['code'] = exc.status_code
        err_template['error']['reason'] = f'You have exceeded the total \
number of allowed requests to this endpoint: {exc.detail}'
        err_template['error']['errors'] = [{
            "error_message": "Too many requests",
            "extended_help": Config['RATE_LIMIT_EXTENDED_HELP']
        }]

        return JSONResponse(
            status_code=exc.status_code,
            content=err_template
        )

    @app.exception_handler(500)
    async def server_error_handler(request: Request, exc: HTTPException):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = 'Unknown server error'
        err_template['error']['code'] = 500
        err_template['error']['reason'] = 'Internal Server Error'
        err_template['error']['errors'] = [{
            "error_message": "Unexpected error occurred when attempting to process this request",
            "extended_help": Config['SERVER_ERROR_EXTENDED_HELP']
        }]

        return JSONResponse(
            status_code=500,
            content=err_template
        )

    # Octy HTTP Exceptions (exceptions with OctyException attributes)
    @app.exception_handler(OctyException)
    async def octy_exception_handler(request: Request, exc: OctyException):

        err_template = deepcopy(Config['ERROR_TEMPLATE'])
        err_template['request_meta']['message'] = status_code_detail_map[exc.code]
        err_template['error']['code'] = exc.code
        err_template['error']['reason'] = exc.error_description

        for error in exc.reasons:
            err_template['error']['errors'].append(error)

        return JSONResponse(
            status_code=exc.code,
            content=err_template,
        )
