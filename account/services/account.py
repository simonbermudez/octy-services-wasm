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
import time

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
            True if account was deleted successfully, False otherwise : bool
        """
        print(f"=== Starting account deletion process for account_id: {account_id} ===")
        
        try:
            # Delete bucket
            print("Step 1: Initializing bucket repository...")
            bucket_repo = BucketRepository(account_id)
            print("Bucket repository initialized successfully")

            print("Step 2: Retrieving account information...")
            try:
                account = await accountRepository.get_account_by_account_id(account_id)
                print(f"Account retrieved successfully: {account}")
                print(f"Account bucket: {account.bucket if hasattr(account, 'bucket') else 'No bucket attribute'}")
            except Exception as e:
                print(f"ERROR: Failed to retrieve account: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise

            print("Step 3: Deleting bucket...")
            try:
                res = bucket_repo.delete_bucket(account.bucket)
                print(f"Bucket deletion result: {res}")
                
                if res is False:
                    print("ERROR: Bucket deletion failed")
                    raise Exception(500, 'Bucket could not be deleted.')
                print("Bucket deleted successfully")
            except Exception as e:
                print(f"ERROR: Exception during bucket deletion: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise

            print("Step 4: Deleting account from database...")
            try:
                res = accountRepository.delete_account(account_id)
                print(f"Account deletion from database result: {res}")
            except Exception as e:
                print(f"ERROR: Failed to delete account from database: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise

            print("Step 5: Preparing payload for service cleanup...")
            payload = {
                'account_id': account_id
            }
            print(f"Payload prepared: {payload}")

            print("Step 6: Starting service cleanup requests...")
            
            # Events service cleanup
            print("Step 6.1: Cleaning up events service...")
            try:
                events_url = f"{Config['EVENTS_SERVICE_CLUSTER_IP']}/v1/internal/events/delete"
                print(f"Sending request to events service: {events_url}")
                await self._send_http_request(url=events_url, payload=payload)
                print("Events service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Events service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Events service cleanup failed: {str(e)}')

            # Profiles service cleanup
            print("Step 6.2: Cleaning up profiles service...")
            try:
                profiles_url = f"{Config['PROFILES_SERVICE_CLUSTER_IP']}/v1/internal/profiles/delete"
                print(f"Sending request to profiles service: {profiles_url}")
                await self._send_http_request(url=profiles_url, payload=payload)
                print("Profiles service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Profiles service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Profiles service cleanup failed: {str(e)}')

            # Octy jobs service cleanup
            print("Step 6.3: Cleaning up octy jobs service...")
            try:
                jobs_url = f"{Config['OCTY_JOBS_SERVICE_CLUSTER_IP']}/v1/internal/jobs/delete"
                print(f"Sending request to octy jobs service: {jobs_url}")
                await self._send_http_request(url=jobs_url, payload=payload)
                print("Octy jobs service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Octy jobs service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Octy jobs service cleanup failed: {str(e)}')

            # Items service cleanup
            print("Step 6.4: Cleaning up items service...")
            try:
                items_url = f"{Config['ITEMS_SERVICE_CLUSTER_IP']}/v1/internal/items/delete"
                print(f"Sending request to items service: {items_url}")
                await self._send_http_request(url=items_url, payload=payload)
                print("Items service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Items service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Items service cleanup failed: {str(e)}')

            # Recommendation service cleanup
            print("Step 6.5: Cleaning up recommendation service...")
            try:
                recommendation_url = f"{Config['RECOMMENDATION_SERVICE_CLUSTER_IP']}/v1/internal/recommendations/delete"
                print(f"Sending request to recommendation service: {recommendation_url}")
                await self._send_http_request(url=recommendation_url, payload=payload)
                print("Recommendation service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Recommendation service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Recommendation service cleanup failed: {str(e)}')

            # Segmentation service cleanup
            print("Step 6.6: Cleaning up segmentation service...")
            try:
                segmentation_url = f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments/delete"
                print(f"Sending request to segmentation service: {segmentation_url}")
                await self._send_http_request(url=segmentation_url, payload=payload)
                print("Segmentation service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Segmentation service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Segmentation service cleanup failed: {str(e)}')

            # Churn prediction service cleanup
            print("Step 6.7: Cleaning up churn prediction service...")
            try:
                churn_url = f"{Config['CHURN_PREDICTION_SERVICE_CLUSTER_IP']}/v1/internal/churn_prediction/delete"
                print(f"Sending request to churn prediction service: {churn_url}")
                await self._send_http_request(url=churn_url, payload=payload)
                print("Churn prediction service cleanup completed successfully")
            except Exception as e:
                print(f"ERROR: Churn prediction service cleanup failed: {str(e)}")
                print(f"Exception type: {type(e).__name__}")
                raise Exception(500, f'Churn prediction service cleanup failed: {str(e)}')

            print("Step 7: Validating account deletion result...")
            if not res:
                print("ERROR: Account deletion result was False or None")
                raise Exception(500, 'Account could not be deleted.')
            
            print("=== Account deletion completed successfully ===")
            return True

        except Exception as e:
            print(f"=== FATAL ERROR in delete_account: {str(e)} ===")
            print(f"Exception type: {type(e).__name__}")
            print("Account deletion process failed")
            raise

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
        print("=== Starting account creation process ===")
        
        try:
            bucket_name = generate_uid('bucket')
            print(f"Generated bucket name: {bucket_name}")

            # Create account
            print("Creating account in database...")
            # TODO : Probably need to change this to run after creation of bucket 
            new_account, sk = await accountRepository.create_account(account, bucket_name)
            print(f'New account created: {new_account["account_id"]}')

            # Create and configure bucket
            print("Initializing bucket repository...")
            bucket_repo = BucketRepository(new_account)
            print("Bucket repository initialized successfully")

            print(f"Creating bucket: {bucket_name}")
            res = bucket_repo.create_bucket(bucket_name)
            if not res:
                print('ERROR: Bucket could not be created.')
                print("Cleaning up - deleting account...")
                await accountRepository.delete_account(new_account["account_id"])
                raise Exception(500, 'Bucket could not be created.')
            print("Bucket created successfully")

            print(f"Configuring bucket: {bucket_name}")
            res = bucket_repo.bucket_configuration(bucket_name)
            if not res:
                print('ERROR: Bucket could not be configured.')
                print("Cleaning up - deleting account...")
                await accountRepository.delete_account(new_account["account_id"])
                raise Exception(500, 'Bucket could not be configured')
            print("Bucket configured successfully")

            # Create required directories
            print("Creating required directories...")
            for dir in Config['BUCKET_REQUIRED_DIRS']:
                print(f"Creating directory: {dir}")
                bucket_repo.create_directory(bucket_name, dir)
            print("All required directories created")

            # send email notification
            print("Sending email notification...")
            notification_sent = await NotificationsRepository(account=new_account) \
                .email(
                {
                    'contact_email_address': new_account["account_configurations"]["contact_email_address"],
                    'contact_name': new_account["account_configurations"]["contact_name"],
                    'subject': ACCOUNT_SUBJECT,
                    'body': ACCOUNT_BODY.format(
                        first_name=new_account["account_configurations"]["contact_name"],
                        link=Config['DOCS_ROOT_URL'],
                        pk=new_account["keys"]["public_key"],
                        sk=sk)
                }
            )
            print(f"Email notification sent: {notification_sent}")

            # call amqp service to create Octy jobs
            print("Creating Octy jobs...")
            for job in Config['OCTY_JOBS']:
                print(f"Sending job creation message for job: {job.get('job_meta', {}).get('name', 'unknown')}")
                await amqpPublisher.send_message(routing_key='octy.job.cmd.create',
                                                payload={
                                                    'account_id': new_account['account_id'],
                                                    'job_meta': job['job_meta'],
                                                    'job_data': job['job_data']
                                                })
            print("All Octy jobs created successfully")

            print("=== Account creation completed successfully ===")
            return {
                'account_id': new_account["account_id"],
                'account_name': new_account["account_name"],
                'account_type': new_account["account_configurations"]["account_type"],
                'account_currency': new_account["account_configurations"]["account_currency"],
                'contact_email_address': new_account["account_configurations"]["contact_email_address"],
                'pk': new_account["keys"]["public_key"],
                'sk': sk,
                'notification_sent': notification_sent
            }
            
        except Exception as e:
            print(f"=== ERROR in account creation: {str(e)} ===")
            print(f"Exception type: {type(e).__name__}")
            raise

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
