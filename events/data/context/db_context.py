#module imports 
from config import Config
from app_secrets import Secrets

#python imports
from typing import *

#external imports
from motor.motor_asyncio import AsyncIOMotorClient


class ContextManager():
    """
        ContextManager
        Handles:
        - Database connections
            - connecting
            - disconnecting
        ...

        Attributes
        ----------
        none
    """
    def __init__(self):pass

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

contextManager = ContextManager()