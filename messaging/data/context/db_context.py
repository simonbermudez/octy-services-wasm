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
            logger : logger instance

            Returns
            ----------
            result : None
        """

        for db in Config['DATABASES_ALIASES']:
            connect(host=Secrets[db], alias=db)
            logger.info(f'Opened connection to {db}')

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
        for db in Config['DATABASES_ALIASES']:
            disconnect(host=Secrets[db], alias=db)
            logger.info(f'Closed connection to {db}')

contextManager = ContextManager()