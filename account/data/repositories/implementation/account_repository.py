# module imports
from data.repositories.Iaccount_repository import AccountInterface
from data.models.db_schemas import tbl_accounts, Keys, AccountConfigurations, ChurnInfo, AlgorithmConfigurations
from data.models.account import UpdateAccount
from utils.utils import *
from api.routers.error_handlers import *
from config import Config
import data.context.db_context as ctx

# python imports
from typing import *
import json
from datetime import datetime as dt

# external imports
from mongoengine.errors import NotUniqueError, DoesNotExist
from argon2 import PasswordHasher
from mongoengine.queryset.visitor import Q


class _AccountRepository(AccountInterface):
    """
        _AccountRepository
        Handles:
        - Creation of account in DB

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    #Implemented By Munashe
    def get_account_by_account_id(self, account_id : str) -> object:
        """
            A method used to get an Octy account

            Parameters
            ----------
            account_id : str
                Octy generated account unique identifier

            Returns
            ----------
            tbl_account object : Mongo Document object/ dict
        """
        return tbl_accounts.objects(account_id=account_id).first()   


    def create_account(self, account, bucket : str) -> Union[object, str]:
        """
            A method used to create an Octy account in a mongoDB instance

            Parameters
            ----------
            account : CreateAccount
                CreateAccount request model instance

            bucket : str
                Bucket unique indentifier 

            Returns
            ----------
            New tbl_account object : object
            Secret key : str
        """
        # Argon2 hash secret key
        ph = PasswordHasher()
        secret_key = generate_uid('sk')
        pk = generate_uid('pk')
        keys = Keys(
            public_key = pk,
            secret_key = ph.hash(secret_key)
        )

        # check if account contains authenticated_id_key and set it if it does 
        if account.authenticated_id_key is not None:
            account_configurations = AccountConfigurations(
            account_type=account.account_type,
            account_currency = account.account_currency,
            contact_name = account.contact_name,
            contact_surname = account.contact_surname,
            contact_email_address = account.contact_email_address,
            webhook_url = account.webhook_url,
            authenticated_id_key = account.authenticated_id_key,
            limits = [Config['RESOURCE_LIMITS'][account.account_type]]
                )
        else:
            account_configurations = AccountConfigurations(
            account_type=account.account_type,
            account_currency = account.account_currency,
            contact_name = account.contact_name,
            contact_surname = account.contact_surname,
            contact_email_address = account.contact_email_address,
            webhook_url = account.webhook_url,
            limits = [Config['RESOURCE_LIMITS'][account.account_type]]
        )

        # create algorithm configs base models for each required configuration
        rec_algorithm_configs = AlgorithmConfigurations(
            algorithm_name = 'rec',
            config_json = {}
        )
        churn_algorithm_configs = AlgorithmConfigurations(
            algorithm_name = 'churn',
            config_json = {}
        )

        account_id=generate_uid('account')
        new_account = tbl_accounts(
            account_id=generate_uid('account'),
            account_name=account.account_name,
            bucket=bucket,
            permissions=account.permissions,
            keys=keys,
            account_configurations=account_configurations,
            algorithm_configurations=[rec_algorithm_configs,churn_algorithm_configs],
            churn_info=ChurnInfo(),
            last_updated_action="Account created"
        )

        try:
            new_account.save()
        except NotUniqueError as err:
            raise OctyException(400, 'Duplicate entry', [{'error_message' : str(err), 'extended_help': ''}])

        a = json.loads(new_account.to_json())
        # add top level API usage property to account cache only.
        a['api_usage'] = [
            {
                'month' : 0,
                'request_count' : 0
            }
        ]

        try:
            _cache_account_data(pk=pk, account_data=json.dumps(a))
        except Exception as e:
            # Suggested implementation to handle exception.
            # try:
            #     new_account = tbl_accounts.objects.get(account_id__exact=account_id)
            #     new_account.delete()
            # except Exception as err:
            #     raise Exception(err)
            # raise Exception from e

            new_account = tbl_accounts.objects.get(account_id__exact=account_id)
            new_account.delete()

        return new_account, secret_key

    def get_account(self, pk : str, dict : bool) -> object:
        """
            A method used to get an Octy account

            Parameters
            ----------
            pk : str
                Octy generated account public key

            dict : bool
                Whether the return account as dict object

            Returns
            ----------
            tbl_account object : Mongo Document object/ dict
        """
        if dict:
            return json.loads(tbl_accounts.objects
                                 .get(keys__public_key__exact=pk).to_json())

        return tbl_accounts.objects.get(keys__public_key__exact=pk)

    def get_accounts(self, account_ids : list, cursor : int):
        """
            A method used to get all Octy accounts. paginated.

            Parameters
            ----------
            account_ids : list
            cursor : int

            Returns
            ----------
            :rtype: list
            :rtype: int
        """
        accounts = tbl_accounts.objects((Q(account_id__in=account_ids) & Q(active__exact=True) )).skip(cursor).limit(100)
        total = tbl_accounts.objects((Q(account_id__in=account_ids) & Q(active__exact=True) )).count()

        found_accounts=[]
        for account in accounts:
            account_dict = json.loads(account.to_json())
            account_dict.pop('keys', None)
            found_accounts.append(account_dict)
        
        return found_accounts, total

    async def update_account(self, account : UpdateAccount, action : str):
        """
            A method used to update an Octy account

            Parameters
            ----------
            account : UpdateAccount
                UpdateAccount request model instance
            action : str
                Define which parts of account should be updated

            Returns
            ----------
            None
        """
        try:
            a = tbl_accounts.objects.get(account_id__exact=account.account_id)
        except DoesNotExist as e:
            raise Exception(f"[toxic]:: {e}")

        if action == 'account-config':
            a.account_configurations.contact_name = account.contact_name
            a.account_configurations.contact_surname=account.contact_surname
            a.account_configurations.contact_email_address=account.contact_email_address
            a.account_configurations.webhook_url=account.webhook_url
            a.account_configurations.authenticated_id_key=account.authenticated_id_key

            a.last_updated_action = 'updated account configurations'
            a.updated_at = dt.now()

        elif action == 'algorithm-config':
            #NOTE: rec configs at index 0, churn configs at index 1
            if account.algorithm_configurations.algorithm_name == 'rec':
                idx = 0
            elif account.algorithm_configurations.algorithm_name == 'churn':
                idx = 1
            a.algorithm_configurations[idx].algorithm_name = account.algorithm_configurations.algorithm_name
            a.algorithm_configurations[idx].config_json = account.algorithm_configurations.config_json

            a.last_updated_action = 'updated algorithm configurations'
            a.updated_at = dt.now()


        elif action == 'churn-info':
            a.churn_info.churn_percentage = account.churn_info.churn_percentage
            a.churn_info.churn_indicator = account.churn_info.churn_indicator
            a.churn_info.churn_difference = account.churn_info.churn_difference
            a.churn_info.features = account.churn_info.features

            a.last_updated_action = 'updated churn info'
            a.updated_at = dt.now()

        a.save()

        res = ctx.redis_conn.get(f'pk:{a.keys.public_key}')
        if not res:
            raise Exception(f"[toxic]:: Account not found in DB cache!")
        acc_cache = json.loads(res)

        acc = json.loads(json.dumps(a.to_mongo(), default=json_serial))
        # add top level API usage property to account cache only.
        try:
            acc['api_usage'] = acc_cache['api_usage']
        except KeyError as e:
            raise Exception(f"[toxic]:: {e}")

        _cache_account_data(pk=a.keys.public_key, account_data=json.dumps(acc))

    def delete_account(self, account_id : str) -> None:
        """
            A method used to delete an Octy account

            Parameters
            ----------
            account_id : str
                Octy generated unique account identifier

            Returns
            ----------
            None
        """
        a = tbl_accounts.objects.get(account_id__exact=account_id)
        # Remove account from cache
        ctx.redis_conn.delete(f'pk:{a.keys.public_key}')
        # Delete account from mongo DB
        a.delete()

    async def refresh_account_data_cache(self, pk: str) -> None:
        """
            A method used to refresh account data by fetching from MongoDB and updating cache

            Parameters
            ----------
            pk : str
                Octy generated account public key

            Returns
            ----------
            None
        """
        try:
            account = tbl_accounts.objects.get(keys__public_key__exact=pk)
        except DoesNotExist as e:
            print(f"Account not found for pk: {pk}")
            return

        # Convert account document to JSON dictionary
        account_dict = json.loads(account.to_json())
        account_dict.pop('keys', None)  

        # Update the account data in cache
        _cache_account_data(pk=pk, account_data=json.dumps(account_dict))
        
        print(f"Refreshed account data for pk: {pk}")    

    async def update_account_cache(self, account : dict) -> None:
        """
            Parameters
            ----------
            account : dict
                Octy account

            Returns
            ----------
            :rtype: None
        """
        _cache_account_data(pk=account['keys']['public_key'], account_data=json.dumps(account))

# Private functions

def _cache_account_data(pk : str, account_data : dict) -> None:
    ctx.redis_conn.set(f'pk:{pk}', account_data)

accountRepository = _AccountRepository()