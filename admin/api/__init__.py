#module imports 
from .routers import admin
from .routers.error_handlers import add_exception_handlers
from config import Config

#python imports

#external imports
from fastapi import FastAPI, Request
import sentry_sdk


app = FastAPI()
@app.on_event('startup')
async def startup():
    sentry_sdk.init(
    Config['SENTRY_URL'],
    traces_sample_rate=1.0,
    environment=Config['ENV'],
)

add_exception_handlers(app)
app.include_router(admin.router)
