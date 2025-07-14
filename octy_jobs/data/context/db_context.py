#module imports 

# data context for octy_jobs
from config import Config
from app_secrets import Secrets

#python imports
from typing import *

#external imports
# from mongoengine import connect, disconnect
from motor.motor_asyncio import AsyncIOMotorClient
import redis.asyncio as redis, certifi

redis_conn = None

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
    def __init__(self):
        self.mongo_client = None
        self.db = None

    async def db_connect(self, logger) -> None:
        self.mongo_client = AsyncIOMotorClient(Secrets["DB_URI"])
        self.db = self.mongo_client[Config["DB_NAME"]]
        logger.info("Opened connection to MongoDB")

    async def db_disconnect(self, logger) -> None:
        self.mongo_client.close()
        logger.info("Closed connection to MongoDB")

    # async def db_connect(self, logger) -> None: 
    #     """
    #         A method used to connect to a mongoDB database

    #         Parameters
    #         ----------
    #         logger : logger instance

    #         Returns
    #         ----------
    #         result : None
    #     """

    #     connect(host=Secrets["DB_URI"])
    #     logger.info("Opened connection to DB")

    # async def db_disconnect(self, logger) -> None: 
    #     """
    #         A method used to disconnect from a mongoDB database

    #         Parameters
    #         ----------
    #         logger : logger instance

    #         Returns
    #         ----------
    #         result : None
    #     """

    #     #Disconnect from mongoDB
    #     disconnect(alias=Config["DB_ALIAS"])
    #     logger.info("Closed conenction to DB")

    async def db_redis_connect(self, logger) -> None: 
        """
            A method used to connect to a redis database

            Parameters
            ----------
            None

            Returns
            ----------
            None
        """

        global redis_conn
        redis_conn = \
            redis.Redis(
                    host=Config['REDIS_PUB_HOST'], 
                    port=Config['REDIS_PORT'], 
                    password=Secrets['REDIS_PASS'],
                    db=3,
                    ssl=True, 
                    ssl_ca_certs=certifi.where())
        logger.info(f'Opened Redis connection pool. host: {Config["REDIS_PUB_HOST"]} on port: {Config["REDIS_PORT"]}')


contextManager = ContextManager()

