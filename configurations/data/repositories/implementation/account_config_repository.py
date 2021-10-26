# module imports
from data.repositories.Iaccount_config_repository import AccountConfigInterface
from config import Config
from octy_rabbitmq.amqp_publisher import amqpPublisher

# python imports
import requests
from typing import *

# external imports


class _AccountConfigRepository(AccountConfigInterface):
    """
        _AccountConfigRepository
        Handles:
        - Updating account configurations

        ...

        Attributes
        ----------
        ...
    """
    def __init__(self): pass


    async def set_account_configs(self, account : object) -> None:
        """
            Parameters
            ----------
            account_config : SetAccountConfigs
                Updated account configurations

            Returns
            ----------
            None
        """
        await amqpPublisher.send_message(routing_key='account.configs.cmd.update',
        payload={
            "account_id" : account.account_id,
            "contact_email_address" : account.contact_email_address,
            "contact_name" : account.contact_name,
            "contact_surname" : account.contact_surname,
            "webhook_url" : account.webhook_url,
            "authenticated_id_key" : account.authenticated_id_key
        })


accountConfigRepository = _AccountConfigRepository()