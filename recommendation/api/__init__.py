#module imports 
from .routers import recommendation
from .routers.error_handlers import add_exception_handlers
from config import *
from data.context.db_context import contextManager

#python imports
import logging


#external imports
from fastapi import FastAPI
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn')

@app.on_event("startup")
async def startup():
    # Connect to mongoDB
    contextManager.db_connect()

    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],)

@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    contextManager.db_disconnect()

add_exception_handlers(app)
app.include_router(recommendation.router)