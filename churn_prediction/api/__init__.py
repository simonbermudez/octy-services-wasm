#module imports 
from .routers import churn_prediction
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
    await contextManager.db_connect(logger=logger)

    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],)

@app.on_event("shutdown")
async def shutdown():
    # Disconnect from mongoDB
    await contextManager.db_disconnect(logger=logger)
    
add_exception_handlers(app)
app.include_router(churn_prediction.router)
