# module imports
from data.repositories.implementation.account_repository import accountRepository
from data.repositories.implementation.bucket_repository import BucketRepository
from data.repositories.implementation.notifications_repository import NotificationsRepository
from data.repositories.content.notification_content import ACCOUNT_SUBJECT, ACCOUNT_BODY
from api.routers.request_models.account import *
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports


# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import Request

from api.routers.error_handlers import OctyException


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

    def __init__(self):
        pass

    async def delete_account(self, account_id: str) -> bool:
        """
            A method used to delete an Octy account.

            Parameters
            ----------

            account_id : str
                Account unique identifier

            Returns
            ----------
            erTrue if account was deleted successfully, False otherwise : bool
        """

        # Delete bucket
        bucket_repo = BucketRepository(account_id)
        account = accountRepository.get_account_by_account_id(account_id)
        res = bucket_repo.delete_bucket(account.bucket)
    
        if res is False:
            raise Exception(500, 'Bucket could not be deleted.')
        
        # Delete account
        res = accountRepository.delete_account(account_id)

        try:
            await amqpPublisher.send_message(routing_key='octy.job.cmd.delete',
                                             payload={
                                                 'account_id': account_id
                                             })
            payload = {
           'account_id': account_id
            }
            await _send_http_request(url=f"{Config['BILLING_SERVICE_CLUSTER_IP']}/v1/internal/billing/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['EVENTS_SERVICE_CLUSTER_IP']}/v1/internal/events/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['EVENTS_SERVICE_CLUSTER_IP']}/v1/retention/events/types/delete/all",
                                     payload=payload)
            await _send_http_request(url=f"{Config['PROFILES_SERVICE_CLUSTER_IP']}/v1/internal/profiles/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['OCTY_JOBS_SERVICE_CLUSTER_IP']}/v1/internal/octy-jobs/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['ITEMS_SERVICE_CLUSTER_IP']}/v1/internal/items/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['RECOMMENDATION_SERVICE_CLUSTER_IP']}/v1/internal/recommendations/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments/delete",
                                     payload=payload)
            await _send_http_request(url=f"{Config['CHURN_PREDICTION_SERVICE_CLUSTER_IP']}/v1/internal/churn_prediction/delete",
                                     payload=payload)

        except Exception as e:
            raise Exception(500, 'Account Data could not be deleted.')


        if not res:
            raise Exception(500, 'Account could not be deleted.')   

        return True

    async def create_account(self, account: CreateAccount) -> Dict:
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
        # TODO : Probably need to change this to run after creation of bucket 
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
        notification_sent = NotificationsRepository(account=new_account) \
            .email(
            {
                'contact_email_address': new_account.account_configurations.contact_email_address,
                'contact_name': new_account.account_configurations.contact_name,
                'subject': ACCOUNT_SUBJECT,
                'body': ACCOUNT_BODY.format(
                    first_name=new_account.account_configurations.contact_name,
                    link=Config['DOCS_ROOT_URL'],
                    pk=new_account.keys.public_key,
                    sk=sk)
            }
        )

        # call amqp service to create Octy jobs
        for job in Config['OCTY_JOBS']:
            await amqpPublisher.send_message(routing_key='octy.job.cmd.create',
                                             payload={
                                                 'account_id': new_account['account_id'],
                                                 'job_meta': job['job_meta'],
                                                 'job_data': job['job_data']
                                             })

        return {
            'account_id': new_account.account_id,
            'account_name': new_account.account_name,
            'account_type': new_account.account_configurations.account_type,
            'account_currency': new_account.account_configurations.account_currency,
            'contact_email_address': new_account.account_configurations.contact_email_address,
            'pk': new_account.keys.public_key,
            'sk': sk,
            'notification_sent': notification_sent
        }

    async def get_accounts_internal(self, account_ids: list, cursor: int) -> list:
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
        if len(accounts) < 1:
            raise OctyException(400, 'No accounts found',
                                [{
                                     'error_message': 'No accounts found with provided params or pagination cursor exhausted',
                                     'extended_help': ''}])
        return accounts, total

    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                headers={'cursor': str(0)},
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0}seconds')

accountService = AccountService()
