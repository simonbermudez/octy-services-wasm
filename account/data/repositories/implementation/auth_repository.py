# module imports
from data.repositories.Iauth_repository import AuthInterface
from data.models.db_schemas import tbl_accounts, tbl_failed_auth_attempts
from secrets import Secrets
from utils.utils import dt_to_int, base64_decode

# python imports
from datetime import datetime as dt
from datetime import timezone as tz
from datetime import timedelta as td
import json
from typing import *
import os

# external imports
import jwt
from mongoengine.errors import DoesNotExist
from mongoengine.queryset.visitor import Q
from argon2 import PasswordHasher
from argon2.exceptions import VerifyMismatchError


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
    def __init__(self): pass


    def verify_account_keys(self, pk: str, sk: str) -> Union[bool, bool]:
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
        """
        
        try:
            account = tbl_accounts.objects.get(keys__public_key__exact=pk)
            if account.active != True:
                return False, False
        except DoesNotExist:
            return False, False

        #Argon2 secret key for comparrison
        ph = PasswordHasher()
        try:
            ph.verify(account.keys.secret_key, sk)
        except VerifyMismatchError:
            return True, False

        return True, True

    async def auth_token(self, pk: str) -> str:
        """
            A method used to generate a fat jwt token,
            containing account information + authorized tags

            Parameters
            ----------
            pk : str
                Octy public key

            Returns
            ----------
            auth jwt token : str
        """
        account = json.loads(tbl_accounts.objects
                             .get(keys__public_key__exact=pk).to_json())

        # with open('./account/keys/octy-private-key', 'rb') as f:
        #     private_key = f.read()
        try:
            private_key = base64_decode(os.environ.get('OCTY_PRIVATE_KEY'), is_json=False)
        except:
            private_key = os.environ.get('OCTY_PRIVATE_KEY')
        
        def _val_or_none(obj, key):
            try:
                return obj[key]
            except:
                return None

        #Abbreviate keys to reduce the size of JWT tokens
        payload = {
            "m" : {
                "iss": "octy-auth-service",
                "iat": dt_to_int(dt.now(tz.utc)),
                "exp": dt_to_int(dt.now(tz.utc) + td(hours=1))
            },
            "b" : {
                "a_id" : account['_id'],
                "a_n" : account['account_name'],
                "b" : account['bucket'],
                "pe" : account['permissions'],
                "a_cf" : {
                    "c_n": account['account_configurations']['contact_name'],
                    "c_s": account['account_configurations']['contact_surname'],
                    "c_e": account['account_configurations']['contact_email_address'],
                    "we": account['account_configurations']['webhook_url'],
                    "ak": _val_or_none(account['account_configurations'],'authenticated_id_key'),
                    "li": f"{account['account_configurations']['limits'][0]['MAX_TOTAL_PROFILES']}*\
{account['account_configurations']['limits'][0]['MAX_TOTAL_ITEMS']}*\
{account['account_configurations']['limits'][0]['MAX_TOTAL_CUSTOM_EVENT_TYPES']}*\
{account['account_configurations']['limits'][0]['MAX_TOTAL_EVENTS']}*\
{account['account_configurations']['limits'][0]['MAX_TOTAL_SEGMENT_DEFINITIONS']}*\
{account['account_configurations']['limits'][0]['MAX_TOTAL_MESSAGE_TEMPLATES']}",
                },
                "al_cf" : account['algorithm_configurations'],
                "c_i" : account['churn_info'],
                "c_at" : account['created_at']
            }
        }

        jwt_token = jwt.encode(payload, private_key, algorithm='RS256')

        return jwt_token


    def log_failed_auth(self, failed_attempt : Dict) -> object:
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

        # log new failed auth
        tbl_failed_auth_attempts(
            public_key = failed_attempt['public_key'],
            user_agent = failed_attempt['user_agent'],
            server_name = failed_attempt['server_name'],
            server_port = failed_attempt['server_port'],
            request_type = failed_attempt['request_type']
        ).save()

        # get all failed auth attempts that occurred in the last x minutes.
        backdate = dt.now() - td(minutes=30)

        try:
            return tbl_failed_auth_attempts.objects(
                Q(public_key=failed_attempt['public_key']) & Q(created_at__gt=backdate)
            )
        except DoesNotExist:
            return []



authRepository = _AuthRepository()