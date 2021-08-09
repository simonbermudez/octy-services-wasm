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
    def __init__(self):pass

    def db_connect(self) -> None: 
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
        print('Opened connection to DB')

    def db_disconnect(self) -> None: 
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
        print('Closed conenction to DB')

contextManager = ContextManager()