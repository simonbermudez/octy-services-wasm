#module imports 
from config import Config
from secrets import Secrets

#python imports
from typing import *

#external imports
import redis, certifi
from fastapi import FastAPI

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
    def __init__(self): pass

    async def db_connect(self, logger) -> None: 
        """
            A method used to connect to a redis database

            Parameters
            ----------
            logger

            Returns
            ----------
            result : None
        """
        global redis_conn
        redis_conn = \
            redis.Redis(
                    host=Config['REDIS_PUB_HOST'], 
                    port=Config['REDIS_PORT'], 
                    password=Secrets['REDIS_PASS'],
                    db=0,
                    ssl=True, 
                    ssl_ca_certs=certifi.where())
        logger.info(f'Opened Redis connection pool. host: {Config["REDIS_PUB_HOST"]} on port: {Config["REDIS_PORT"]}')

contextManager = ContextManager()