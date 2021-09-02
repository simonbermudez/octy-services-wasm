#module imports 
from config import Config
from secrets import Secrets

#python imports
from typing import *

#external imports
from mongoengine import connect, disconnect
import redis, certifi

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
    def __init__(self):pass

    def db_connect(self, app, logger) -> None: 
        """
            A method used to connect to a mongoDB database

            Parameters
            ----------
            None

            Returns
            ----------
            result : None
        """

        con = connect(host=Secrets['DB_URI'])
        app.state.mongo_conn = con
        logger.info('Opened connection to DB')

    def db_disconnect(self, logger) -> None: 
        """
            A method used to disconnect from a mongoDB database

            Parameters
            ----------
            None

            Returns
            ----------
            result : None
        """

        #Disconnect from mongoDB
        disconnect(alias=Config['DB_ALIAS'])
        logger.info('Closed conenction to DB')

    def db_redis_connect(self, logger) -> None: 
        """
            A method used to connect to a redis database

            Parameters
            ----------
            None

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
                    db=1,
                    ssl=True, 
                    ssl_ca_certs=certifi.where())
        logger.info(f'Opened Redis connection pool. host: {Config["REDIS_PUB_HOST"]} on port: {Config["REDIS_PORT"]}')

contextManager = ContextManager()