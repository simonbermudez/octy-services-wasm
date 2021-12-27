#module imports 
from .routers import billing, healthz
from .routers.error_handlers import add_exception_handlers
from config import Config
from data.context.db_context import contextManager
from amqp.consumer import on_consumer_message_cb

#python imports
import logging

#external imports
from fastapi import FastAPI, Request
from octy_rabbitmq.amqp_consumer import AMQPConsumer
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn.error')

class HealthCheckFilter(logging.Filter):
    def filter(self, record):
        return record.getMessage().find("/healthz") == -1

@app.on_event('startup')
async def startup():
    sentry_sdk.init(
        Config['SENTRY_URL'],
        traces_sample_rate=1.0,
        environment=Config['ENV'],
    )
    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)
    '''
    AMQP CONSUMERS
    '''
    await AMQPConsumer(Config['EXCHANGE'],
        Config['AMQP_URL'], 
        Config['AMQP_CONSUMERS'], 
        on_consumer_message_cb, 
        logger).start() # Start consumers

@app.on_event('shutdown')
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)

add_exception_handlers(app)
app.include_router(billing.router)
app.include_router(healthz.router)
