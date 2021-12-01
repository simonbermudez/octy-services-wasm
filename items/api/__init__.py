#module imports 
from .routers import items, healthz
from .routers.error_handlers import add_exception_handlers
from config import Config
from data.context.db_context import contextManager

#python imports
import logging

#external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import FastAPI, Request
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')

class HealthCheckFilter(logging.Filter):
    def filter(self, record):
        return record.getMessage().find("/healthz") == -1

@app.on_event('startup')
async def startup():
    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'])

    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)

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


@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)


add_exception_handlers(app)
app.include_router(items.router)
app.include_router(healthz.router)
