# module imports
from data.repositories.Irecommendation_repository import RecommendationsInterface
from data.models.db_schemas import *
from secrets import Secrets
from mongoengine.errors import DoesNotExist

# python imports
from datetime import datetime as dt
import json

# external imports
from bson.json_util import dumps
from mongoengine.errors import BulkWriteError
from sentry_sdk import capture_exception

class _RecommendationsRepository(RecommendationsInterface):
    """
        _RecommendationsRepository
        Handles:
        - Get latest recommendations training job
        - Get cached item recommendations
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def get_latest_hp_tuning_job(self, account_id : str) -> dict:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        hp_tuning_job : dict
        """
        query = {'$and' : [
            {"account_id" : { "$eq" : account_id}},
            {"status" : { "$eq" : 'Completed'}},
        ]}
        results_cursor = tbl_hparam_tuning_jobs._get_collection().find(query).sort('updated_at', -1).limit(1)
        hp_tuning_job = json.loads(dumps(list(results_cursor), indent = 2))
        return hp_tuning_job

    async def get_cached_recommendations(self,
                                account_id : str,
                                training_job_id : str,
                                profile_ids : list) -> dict:
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        profile_ids : list

        Returns
        ----------
        cached item recommendations : dict
        """
        query = {
            '$and' : [
                {"account_id" : { "$eq" : account_id}},
                {"training_job_id" : { "$eq" : training_job_id}},
                {"profile_id" : { "$in" : profile_ids}}
        ]}
        results_cursor = tbl_recommendations_cache._get_collection().find(query)
        recommendations = json.loads(dumps(list(results_cursor), indent = 2))
        return recommendations

    async def delete_cached_recommendations(self,
                                account_id : str,
                                profiles : list):
        """
        Parameters
        ----------
        account_id : str
        profiles : list

        Returns
        ----------
        None
        """
        tbl_recommendations_cache.objects(account_id__exact=account_id, profile_id__in=profiles).delete()

    #delete all recommendations for an account from both cache and tbl_recommendations
    async def delete_all_cached_recommendations(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        bool : True if successful, False otherwise
        """

        try:

            tbl_recommendations_cache.objects(account_id__exact=account_id).delete()
            tbl_hparam_tuning_jobs.objects(account_id__exact=account_id).delete()
            return True
        except Exception as e:
            capture_exception(e)
            return False
        
recommendationsRepository = _RecommendationsRepository()