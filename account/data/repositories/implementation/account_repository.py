# module imports
from data.repositories.Iaccount_repository import AccountInterface
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
from bson import ObjectId, json_util
from typing import Union
from datetime import datetime as dt
from argon2 import PasswordHasher
import json

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
    def __init__(self):
        self.collection = lambda: ctx.contextManager.db["tbl_accounts"]

    async def get_account_by_account_id(self, account_id: str) -> dict:
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
        return await self.collection().find_one({"account_id": account_id})

    async def create_account(self, account, bucket: str) -> tuple[dict, str]:
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
        ph = PasswordHasher()
        secret_key = generate_uid('sk')
        pk = generate_uid('pk')

        keys = {
            "public_key": pk,
            "secret_key": ph.hash(secret_key)
        }

        account_configurations = {
            "account_type": account.account_type,
            "account_currency": account.account_currency,
            "contact_name": account.contact_name,
            "contact_surname": account.contact_surname,
            "contact_email_address": account.contact_email_address,
            "webhook_url": account.webhook_url,
            "authenticated_id_key": account.authenticated_id_key if account.authenticated_id_key else None,
            "limits": [Config["RESOURCE_LIMITS"][account.account_type]]
        }

        rec_algorithm_configs = {
            "algorithm_name": "rec",
            "config_json": {}
        }

        churn_algorithm_configs = {
            "algorithm_name": "churn",
            "config_json": {}
        }

        account_id = generate_uid('account')

        new_account = {
            "account_id": account_id,
            "account_name": account.account_name,
            "bucket": bucket,
            "permissions": account.permissions,
            "keys": keys,
            "account_configurations": account_configurations,
            "algorithm_configurations": [rec_algorithm_configs, churn_algorithm_configs],
            "churn_info": {},
            "last_updated_action": "Account created",
            "connected_platforms": [platform.dict() for platform in account.connected_platforms] if account.connected_platforms else [],
            "created_at": dt.utcnow(),
            "updated_at": dt.utcnow(),
            "active": True
        }

        try:
            await self.collection().insert_one(new_account)
        except Exception as err:
            raise OctyException(400, 'Duplicate entry', [{'error_message': str(err), 'extended_help': ''}])

        new_account['api_usage'] = [{"month": 0, "request_count": 0}]

        try:
            _cache_account_data(pk=pk, account_data=json_util.dumps(new_account))
        except Exception:
            await self.collection().delete_one({"account_id": account_id})
            raise

        return new_account, secret_key

    async def get_account(self, pk: str, as_dict: bool) -> Union[dict, object]:
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
        account = await self.collection().find_one({"keys.public_key": pk})
        if not account:
            return None
        return account if as_dict else account

    async def get_accounts(self, account_ids: list, cursor: int):
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
        query = {"account_id": {"$in": account_ids}, "active": True}
        cursor_obj = self.collection().find(query).skip(cursor).limit(100)
        total = await self.collection().count_documents(query)

        accounts = []
        async for doc in cursor_obj:
            doc.pop('keys', None)
            accounts.append(doc)

        return accounts, total

    async def update_account(self, account: UpdateAccount, action: str):
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
        update_fields = {}
        now = dt.utcnow()

        if action == "account-config":
            update_fields = {
                "account_configurations.contact_name": account.contact_name,
                "account_configurations.contact_surname": account.contact_surname,
                "account_configurations.contact_email_address": account.contact_email_address,
                "account_configurations.webhook_url": account.webhook_url,
                "account_configurations.authenticated_id_key": account.authenticated_id_key,
                "last_updated_action": "updated account configurations",
                "updated_at": now
            }

        elif action == "algorithm-config":
            index = 0 if account.algorithm_configurations.algorithm_name == "rec" else 1
            key_base = f"algorithm_configurations.{index}"
            update_fields = {
                f"{key_base}.algorithm_name": account.algorithm_configurations.algorithm_name,
                f"{key_base}.config_json": account.algorithm_configurations.config_json,
                "last_updated_action": "updated algorithm configurations",
                "updated_at": now
            }

        elif action == "churn-info":
            update_fields = {
                "churn_info.churn_percentage": account.churn_info.churn_percentage,
                "churn_info.churn_indicator": account.churn_info.churn_indicator,
                "churn_info.churn_difference": account.churn_info.churn_difference,
                "churn_info.features": account.churn_info.features,
                "last_updated_action": "updated churn info",
                "updated_at": now
            }

        await self.collection().update_one({"account_id": account.account_id}, {"$set": update_fields})

        acc = await self.collection().find_one({"account_id": account.account_id})
        if not acc:
            raise Exception("Account not found")

        res = ctx.redis_conn.get(f'pk:{acc["keys"]["public_key"]}')
        if not res:
            raise Exception("Account not found in DB cache")

        acc_cache = json.loads(res)
        acc['api_usage'] = acc_cache.get('api_usage', [])
        _cache_account_data(pk=acc["keys"]["public_key"], account_data=json_util.dumps(acc))

    async def delete_account(self, account_id: str):
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
        acc = await self.collection().find_one({"account_id": account_id})
        if not acc:
            return
        ctx.redis_conn.delete(f'pk:{acc["keys"]["public_key"]}')
        await self.collection().delete_one({"account_id": account_id})

    async def refresh_account_data_cache(self, pk: str):
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
        acc = await self.collection().find_one({"keys.public_key": pk})
        if acc:
            _cache_account_data(pk=pk, account_data=json_util.dumps(acc))

    async def update_account_cache(self, account: dict):
        """
            Parameters
            ----------
            account : dict
                Octy account

            Returns
            ----------
            :rtype: None
        """
        _cache_account_data(pk=account['keys']['public_key'], account_data=json_util.dumps(account))


def _cache_account_data(pk: str, account_data: str):
    ctx.redis_conn.set(f'pk:{pk}', account_data)


accountRepository = _AccountRepository()


