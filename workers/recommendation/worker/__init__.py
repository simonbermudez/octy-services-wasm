#module imports 
from .routers import healthz
from config import Config
from data.context.db_context import contextManager
from amqp.consumer import on_consumer_message_cb

#python imports
import logging

#external imports
from octy_rabbitmq.amqp_consumer import AMQPConsumer
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import FastAPI
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

    '''
    AMQP CONSUMERS
    '''
    await AMQPConsumer(Config['EXCHANGE'],
        Config['AMQP_URL'], 
        Config['AMQP_CONSUMERS'], 
        on_consumer_message_cb, 
        logger).start() # Start consumers


@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)

app.include_router(healthz.router)