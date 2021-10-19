#module imports 
from .routers import segmentation
from .routers.error_handlers import add_exception_handlers
from config import Config
from data.context.db_context import contextManager
from services.AMQP import AMQPStateManager

#python imports
import logging


#external imports
from fastapi import FastAPI
import sentry_sdk

#python imports

#external imports
from fastapi import FastAPI, Request
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')

@app.on_event("startup")
async def startup():
    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)

    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],)

    await AMQPStateManager().init_publishers(logger=logger)
    await AMQPStateManager().init_consumers(logger=logger)

@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)
    # graceful disconnection from RabbitMQ
    await app.state.consumer_connection.close_connection()
    await app.state.publisher_connection.close_connection()

add_exception_handlers(app)
app.include_router(segmentation.router)
