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

recommendationsRepository = _RecommendationsRepository()