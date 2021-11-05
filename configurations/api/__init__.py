#module imports 
from .routers import account_configurations, healthz
from .routers import algorithm_configurations
from .routers.error_handlers import add_exception_handlers
from config import Config

#python imports
import logging

#external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import FastAPI
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')

@app.on_event("startup")
async def startup():
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


add_exception_handlers(app)
app.include_router(account_configurations.router)
app.include_router(algorithm_configurations.router)
app.include_router(healthz.router)
