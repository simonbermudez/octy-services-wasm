# module imports
from data.repositories.implementation.account_repository import accountRepository
from data.repositories.implementation.bucket_repository import BucketRepository
from data.repositories.implementation.notifications_repository import NotificationsRepository
from data.repositories.content.notification_content import ACCOUNT_SUBJECT, ACCOUNT_BODY
from api.routers.request_models.account import *
from api.routers.error_handlers import *
from services.AMQP import amqpInterface
from utils.utils import *
from config import Config

# python imports


# external imports
from fastapi import Request


class AccountService:
    """
        AccountService
        Handles:
        - Account creation
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def create_account(self, account : CreateAccount) -> Dict:
        """
            A method used to create an Octy account.

            Parameters
            ----------

            account : CreateAccount Model
                Parsed request body representing a new account

            Returns
            ----------
            new account : Dict
        """

        bucket_name = generate_uid('bucket')

        # Create account
        new_account, sk = accountRepository.create_account(account, bucket_name)

        # Create and configure bucket
        bucket_repo = BucketRepository(new_account)

        res = bucket_repo.create_bucket(bucket_name)
        if not res:
            accountRepository.delete_account(new_account.account_id)
            raise Exception(500, 'Bucket could not be created.')

        res = bucket_repo.bucket_configuration(bucket_name)
        if not res:
            accountRepository.delete_account(new_account.account_id)
            raise Exception(500, 'Bucket could not be configured')

        # Create required directories
        for dir in Config['BUCKET_REQUIRED_DIRS']:
            bucket_repo.create_directory(bucket_name, dir)

        # send email notification
        notification_sent = NotificationsRepository(account=new_account)\
            .email(
                {
                    'contact_email_address' : new_account.account_configurations.contact_email_address,
                    'contact_name' : new_account.account_configurations.contact_name,
                    'subject' : ACCOUNT_SUBJECT,
                    'body' : ACCOUNT_BODY.format(
                        first_name=new_account.account_configurations.contact_name,
                        link=Config['DOCS_ROOT_URL'],
                        pk=new_account.keys.public_key,
                        sk=sk)
                }
        )

        # call amqp service to create Octy jobs
        for job in Config['OCTY_JOBS']:
            await amqpInterface.publish_message(routing_key='octy.job.cmd.create',
                message_payload={
                    'account_id' : new_account['account_id'],
                    'job_type' : job['job_type'],
                    'job_meta' : {
                        'desired_runs' : 0,
                        'time_interval' : 2880, # 2 days
                        'fail_threshold' : 0
                    },
                    'job_data' : job['job_data']
            })


        return {
            'account_id': new_account.account_id,
            'account_name' : new_account.account_name,
            'contact_email_address' : new_account.account_configurations.contact_email_address,
            'pk' : new_account.keys.public_key,
            'notification_sent' : notification_sent
        }
    
    def get_accounts_internal(self, account_ids : list, cursor : int) -> list:
        """
            A method used to get Octy accounts from provided account ids.

            Parameters
            ----------

            account_ids : list
                list of Octy account identifiers

            Returns
            ----------
            accounts : list
            total : int
        """
        accounts, total = accountRepository.get_accounts(account_ids, cursor)
        if len(accounts)<1:
            raise OctyException(400, 'No accounts found', 
                [{'message' : 'No accounts found with provided params or pagination cursor exhausted', 
                'extended_help': ''}])
        return accounts, total


accountService = AccountService()