#module imports 
from config import Config
from secrets import Secrets

#python imports
from typing import *

#external imports
from mongoengine import connect, disconnect


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
            A method used to connect to a mongoDB database

            Parameters
            ----------
            None

            Returns
            ----------
            result : None
        """

        connect(host=Secrets["DB_URI"])
        logger.info('Opened connection to Mongo Database')

    async def db_disconnect(self, logger) -> None: 
        """
            A method used to disconnect from a mongoDB database

            Parameters
            ----------
            logger : logger instance

            Returns
            ----------
            result : None
        """

        #Disconnect from mongoDB
        disconnect(alias=Config['DB_ALIAS'])
        logger.info('Closed conenction to DB')

contextManager = ContextManager()

