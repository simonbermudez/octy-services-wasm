#module imports 
from config import Config
from profiles.app_secrets import Secrets

#python imports
from typing import *

#external imports
# from mongoengine import connect, disconnect
import redis.asyncio as redis
import certifi
from motor.motor_asyncio import AsyncIOMotorClient

redis_conn = None

class ContextManager:
    def __init__(self):
        self.mongo_client = None
        self.db = None

    async def db_connect(self, logger) -> None:
        self.mongo_client = AsyncIOMotorClient(Secrets["DB_URI"])
        self.db = self.mongo_client.get_default_database()
        logger.info("Opened connection to MongoDB")

    async def db_disconnect(self, logger) -> None:
        self.mongo_client.close()
        logger.info("Closed connection to MongoDB")

    async def db_redis_connect(self, logger) -> None:
        global redis_conn
        redis_conn = redis.Redis(
            host=Config['REDIS_PUB_HOST'],
            port=Config['REDIS_PORT'],
            password=Secrets['REDIS_PASS'],
            db=1,
            ssl=True,
            ssl_ca_certs=certifi.where()
        )
        logger.info(f'Opened async Redis connection: {Config["REDIS_PUB_HOST"]}:{Config["REDIS_PORT"]}')

contextManager = ContextManager()