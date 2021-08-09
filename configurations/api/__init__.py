#module imports 
from .routers import account_configurations
from .routers import algorithm_configurations
from services.AMQP import AMQPStateManager
from .routers.error_handlers import add_exception_handlers
from config import Config

#python imports
import logging

#external imports
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

    await AMQPStateManager().init_publishers(logger=logger)


@app.on_event("shutdown")
async def shutdown():
    # graceful disconnection from RabbitMQ
    await app.state.publisher_connection.close_connection()

add_exception_handlers(app)
app.include_router(account_configurations.router)
app.include_router(algorithm_configurations.router)
