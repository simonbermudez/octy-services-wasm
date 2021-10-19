# module imports
from data.repositories.Iprofiles_iden_repository import ProfilesIdenInterface
from data.models.db_schemas import tbl_merged_profiles
from utils.utils import *
from config import Config
import data.context.db_context as ctx

# python imports
from typing import *
import json
from datetime import datetime as dt
import time

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps
from sentry_sdk import capture_exception

class _ProfilesIdenRepository(ProfilesIdenInterface):
    """
        _ProfilesRepository
        Handles:
        - Retrieving profiles
        - Retrieving Profile data key types
        - Creating merged profile references
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def get_profiles(self, account_id : str, status : str = 'active', ids : str = 'false') -> list:
        """
        Parameters
        ----------
        account_id : str
        status : str
        ids : str

        Returns
        ----------
        profiles : list
        """
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids={ids}&status={status}"
        profiles = []
        payload = {
            'account_id': account_id,
            'profiles' : [], 
            'get_all': True
        }
        exhausted_profiles = False

        cursor : int = 0
        session = requests_retry_session()
        while not exhausted_profiles:
            t0 = time.time()
            try:
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=60
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')


            if response.status_code != 200:
                exhausted_profiles = True
                continue

            body = json.loads(response.text)
            for profile in body['profiles']:
                if ids == 'true':
                    profiles.append(
                        profile['profile_id']
                    )
                else:
                    profiles.append(
                        profile
                    )
            cursor +=body['request_meta']['count']

        return profiles

    async def create_merged_profiles_ref(self, merged_profiles : list):
        """
        Parameters
        ----------
        merged_profiles : list

        Returns
        ----------
        :rtype: None
        """
        merged_profile_instances = []
        for profile in merged_profiles:
            merged_profile_instances.append(
                tbl_merged_profiles(
                    account_id=profile['account_id'],
                    merged_profiles=profile['merged_profiles'],
                    parent_profile_id=profile['parent_profile_id'],
                    parent_customer_id=profile['parent_customer_id'],
                    authenticated_id_key=profile['authenticated_id_key'],
                    authenticated_id_value=profile['authenticated_id_value']
                )
            )

        #BULK WRITE OPERATION
        bulk_operation = tbl_merged_profiles._get_collection().initialize_unordered_bulk_op()
        for profile in merged_profile_instances:
            bulk_operation.insert(profile.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            capture_exception(bwe)
            raise Exception('Error occurred when attempting to create merged profile references.')

    def get_profile_key_types(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            octy account id

        Returns
        ----------
        list
        """
        profile_key_types = json.loads(json.dumps([json.loads(s) for s in 
            list(ctx.redis_conn.smembers(f'{account_id}_profile_key_types'))]))
        return profile_key_types

profilesIdenRepository = _ProfilesIdenRepository()

