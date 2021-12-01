#module imports 
from .routers import octy_jobs, healthz
from .routers.error_handlers import add_exception_handlers
from config import *
from data.context.db_context import contextManager
from amqp.consumer import on_consumer_message_cb
from services.octy_jobs import OctyJobQueue

#python imports
import logging


#external imports
from octy_rabbitmq.amqp_consumer import AMQPConsumer
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import FastAPI
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')
octy_job_queue = OctyJobQueue(logger, 2)

class HealthCheckFilter(logging.Filter):
    def filter(self, record):
        return record.getMessage().find("/healthz") == -1

@app.on_event("startup")
async def startup():

    # Connect to redis pool
    await contextManager.db_redis_connect(logger=logger)
    
    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)

    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],)

    '''
    AMQP PUBLISHERS
    '''
    # Import initialised publisher and populate with required attributes
    amqpPublisher.exchange_name = Config['EXCHANGE']
    amqpPublisher.amqp_url = Config['AMQP_URL']
    amqpPublisher.amqp_publishers = Config['AMQP_PUBLISHERS']
    amqpPublisher.logger = logger
    # Start publishers
    await amqpPublisher.start()

    '''
    AMQP CONSUMERS
    '''
    await AMQPConsumer(Config['EXCHANGE'],
        Config['AMQP_URL'], 
        Config['AMQP_CONSUMERS'], 
        on_consumer_message_cb, 
        logger).start() # Start consumers

    await octy_job_queue.start_job_queue()

@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)
    await octy_job_queue.stop_job_queue()

add_exception_handlers(app)
app.include_router(octy_jobs.router)
app.include_router(healthz.router)