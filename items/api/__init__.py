#module imports 
from .routers import items
from .routers.error_handlers import add_exception_handlers
from config import Config
from services.AMQP import AMQPStateManager
from data.context.db_context import contextManager

#python imports
import logging

#external imports
from fastapi import FastAPI, Request
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')

@app.on_event('startup')
async def startup():
    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'])

    # Connect to mongoDB
    contextManager.db_connect()
    await AMQPStateManager().init_publishers(logger=logger)


@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    contextManager.db_disconnect()
    
    # graceful disconnection from RabbitMQ
    await app.state.publisher_connection.close_connection()


add_exception_handlers(app)
app.include_router(items.router)


# @app.middleware('http')
# async def http_middleware(request: Request, call_next):
#     try:
#         # Connect to mongoDB
#         contextManager.db_connect()
#         response = await call_next(request)
#         return response
#     finally:
#         # Disconnect from mongoDB
#         contextManager.db_disconnect()

#     raise Exception(500)
