# module imports
from data.repositories.Ichurn_repository import ChurnPredInterface
from utils.utils import *
from data.models.db_schemas import *
from config import Config
from secrets import Secrets

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import time

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps
from sentry_sdk import capture_exception


class _ChurnPredictionRepository(ChurnPredInterface):
    """
        _ChurnPredictionRepository
        Handles:
        - Get latest training job

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
    
    # delete churn data to do with account_id
    async def delete_account_churn_predictions(self, account_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        True if account churn data was deleted successfully, False otherwise : bool
        """
        query = {'$and' : [
            {"account_id" : { "$eq" : account_id}},
        ]}
        tbl_hparam_tuning_jobs._get_collection().delete_many(query)
        return True


    

churnPredictionRepository = _ChurnPredictionRepository()
