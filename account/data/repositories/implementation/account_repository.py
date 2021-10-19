# module imports
from data.repositories.Iaccount_repository import AccountInterface
from data.models.db_schemas import tbl_accounts, Keys, AccountConfigurations, ChurnInfo, AlgorithmConfigurations
from data.models.account import UpdateAccount
from utils.utils import *
from api.routers.error_handlers import *
from config import Config

# python imports
from typing import *
import json

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
        # Argon2 encrypt secret key
        ph = PasswordHasher()
        secret_key = generate_uid('sk')

        keys = Keys(
            public_key = generate_uid('pk'),
            secret_key = ph.hash(secret_key)
        )
        account_configurations = AccountConfigurations(
            contact_name = account.contact_name,
            contact_surname = account.contact_surname,
            contact_email_address = account.contact_email_address,
            webhook_url = account.webhook_url
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
            raise OctyException(400, 'Duplicate entry', [{'message' : str(err), 'extended_help': ''}])



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
        a = tbl_accounts.objects.get(account_id__exact=account.account_id)

        if action == 'account-config':
            a.account_configurations.contact_name = account.contact_name
            a.account_configurations.contact_surname=account.contact_surname
            a.account_configurations.contact_email_address=account.contact_email_address
            a.account_configurations.webhook_url=account.webhook_url
            a.account_configurations.authenticated_id_key=account.authenticated_id_key

            a.last_updated_action = 'updated account configurations'

        elif action == 'algorithm-config':
            #NOTE: rec configs at index 0, churn configs at index 1
            if account.algorithm_configurations.algorithm_name == 'rec':
                idx = 0
            elif account.algorithm_configurations.algorithm_name == 'churn':
                idx = 1
            a.algorithm_configurations[idx].algorithm_name = account.algorithm_configurations.algorithm_name
            a.algorithm_configurations[idx].config_json = account.algorithm_configurations.config_json

            a.last_updated_action = 'updated algorithm configurations'


        elif action == 'churn-info':
            a.churn_info.churn_precentage = account.churn_info.churn_precentage
            a.churn_info.churn_indicator = account.churn_info.churn_indicator
            a.churn_info.churn_difference = account.churn_info.churn_difference
            a.churn_info.features = account.churn_info.features

            a.last_updated_action = 'updated churn info'

        a.save()

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
        tbl_accounts.objects.get(account_id__exact=account_id).delete()

accountRepository = _AccountRepository()