# module imports
from data.repositories.Iauth_repository import AuthInterface
from app_secrets import Secrets
from utils.utils import dt_to_int, base64_decode
import data.context.db_context as ctx

# python imports
from datetime import datetime as dt
from datetime import timezone as tz
from datetime import timedelta as td
import json
from typing import *
import os

# external imports
import jwt
from argon2 import PasswordHasher
from argon2.exceptions import VerifyMismatchError
from sentry_sdk import capture_exception


class _AuthRepository(AuthInterface):
    """
        AuthRepository
        Handles:
        - Verify account keys
        - Create JWT fat tokens

        ...

        Attributes
        ----------
        none
    """
    def __init__(self):
        self.collection = lambda: ctx.contextManager.db["tbl_failed_auth_attempts"]

    async def verify_account_keys(self, pk: str, sk: str) -> Union[bool, bool, dict]:
        """
            A method used to verify Octy account holder keys

            Parameters
            ----------
            pk : str
                Octy public key
            sk : str
                Octy secret key

            Returns
            ----------
            pk valid : bool
            sk valid : bool
            account : dict | None
        """
        res = ctx.redis_conn.get(f'pk:{pk}')
        if not res:
            return False, False, None
        
        account = json.loads(res)
        if not account['active']:
            return False, False, None

        ph = PasswordHasher()
        try:
            ph.verify(account['keys']['secret_key'], sk)
        except VerifyMismatchError:
            return True, False, None

        return True, True, account

    async def generate_authorization_token(self, account: dict) -> str:
        """
            A method used to generate a fat auth jwt,
            containing account information + authorized tags

            Parameters
            ----------
            account : dict
                Octy account

            Returns
            ----------
            auth jwt : str
        """
        try:
            private_key = base64_decode(os.environ.get('OCTY_PRIVATE_KEY'), is_json=False)
        except:
            private_key = os.environ.get('OCTY_PRIVATE_KEY')

        def _val_or_none(obj, key):
            return obj.get(key)

        payload = {
            "m": {
                "iss": "octy-auth-service",
                "iat": dt_to_int(dt.now(tz.utc)),
                "exp": dt_to_int(dt.now(tz.utc) + td(hours=1))
            },
            "b": {
                "a_id": account['_id'],
                "a_n": account['account_name'],
                "b": account['bucket'],
                "pe": account['permissions'],
                "a_cf": {
                    "a_t": account['account_configurations']['account_type'],
                    "a_c": account['account_configurations']['account_currency'],
                    "c_n": account['account_configurations']['contact_name'],
                    "c_s": account['account_configurations']['contact_surname'],
                    "c_e": account['account_configurations']['contact_email_address'],
                    "we": account['account_configurations']['webhook_url'],
                    "ak": _val_or_none(account['account_configurations'], 'authenticated_id_key'),
                    "li": "*".join(str(account['account_configurations']['limits'][0].get(k)) for k in [
                        'MAX_TOTAL_PROFILES', 'MAX_TOTAL_ITEMS', 'MAX_TOTAL_CUSTOM_EVENT_TYPES',
                        'MAX_TOTAL_EVENTS', 'MAX_TOTAL_SEGMENT_DEFINITIONS', 'MAX_TOTAL_MESSAGE_TEMPLATES'
                    ])
                },
                "al_cf": account['algorithm_configurations'],
                "c_i": account['churn_info'],
                "c_at": account['created_at']
            }
        }

        return jwt.encode(payload, private_key, algorithm='RS256')

    async def log_failed_auth(self, failed_attempt: Dict) -> list:
        """
        Parameters
        ----------
        failed_attempt : Dict
            Dict containing required data to log a failed authentication attempt

        Returns
        ----------
        Logged auth attempts : tbl_failed_auth_attempts object
            All logged auth attmepts that have occurred in the last 30 minutes.
        """
        try:
            await self.collection().insert_one({
                "public_key": failed_attempt['public_key'],
                "user_agent": failed_attempt['user_agent'],
                "server_name": failed_attempt['server_name'],
                "server_port": failed_attempt['server_port'],
                "request_type": failed_attempt['request_type'],
                "created_at": dt.now(tz.utc)
            })

            backdate = dt.now(tz.utc) - td(minutes=30)
            cursor = self.collection().find({
                "public_key": failed_attempt['public_key'],
                "created_at": {"$gt": backdate}
            })
            return await cursor.to_list(length=None)
        except Exception as err:
            capture_exception(err)
            return []

authRepository = _AuthRepository()