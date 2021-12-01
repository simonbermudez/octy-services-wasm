#module imports 
from .routers import admin, healthz
from .routers.error_handlers import add_exception_handlers
from data.context.db_context import contextManager
from config import Config

#python imports
import logging

#external imports
from fastapi import FastAPI, Request
import sentry_sdk


app = FastAPI()
logger = logging.getLogger('uvicorn.error')

class HealthCheckFilter(logging.Filter):
    def filter(self, record):
        return record.getMessage().find("/healthz") == -1

@app.on_event('startup')
async def startup():
    await contextManager.db_connect(logger)
    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],
)

add_exception_handlers(app)
app.include_router(admin.router)
app.include_router(healthz.router)