#module imports 
from .routers import octy_jobs
from .routers.error_handlers import add_exception_handlers
from config import *
from data.context.db_context import contextManager
from services.AMQP import AMQPStateManager
from services.octy_jobs import OctyJobQueue

#python imports
import logging


#external imports
from fastapi import FastAPI
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')
octy_job_queue = OctyJobQueue(logger, 2)

@app.on_event("startup")
async def startup():
    # Connect to mongoDB
    contextManager.db_connect()

    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],)

    await AMQPStateManager().init_consumers(logger=logger)
    await AMQPStateManager().init_publishers(logger=logger)
    await octy_job_queue.start_job_queue()

@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    contextManager.db_disconnect()
    # graceful disconnection from RabbitMQ
    await app.state.consumer_connection.close_connection()
    await app.state.publisher_connection.close_connection()
    await octy_job_queue.stop_job_queue()

add_exception_handlers(app)
app.include_router(octy_jobs.router)