# module imports
from data.repositories.implementation.churn_repository import churnPredictionRepository
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
import json

# external imports
from fastapi import Request


class ChurnPredictionService():
    """
        ChurnPredictionService
        Handles:
        - Generate churn prediction report
        ...

        Attributes
        ----------
        account : Octy account
        account_id : str
    """
    def __init__(self, account : Account): 
        self.account = account

    async def generate_churn_report(self) -> dict:
        """
        Parameters
        ----------

        Returns
        ----------
        churn_report : dict
        """
        training_job = await churnPredictionRepository.get_latest_training_job(account_id=self.account.account_id)
        if not training_job:
            raise OctyException(400,'An error occurred when generating this churn prediction report.', 
                [{'message' : 'No churn prediction training jobs have been completed. Churn prediction training jobs are automatically run every 24 hours', 
                'extended_help': Config['CHURN_PREDICTION_EXTENDED_HELP']}])

        churn_report = {
                'training_job_data' : {
                    'training_job_id' : None,
                    'model_accuracy': 0.0,
                    'training_job_date' : None
                },
                'churn_data' : {
                    'current_churn_percentage' : 0.0,
                    'churn_direction_indication' : None,
                    'churn_percentage_difference': 0.0,
                    'features_of_importance' : []
                }
            }

        meta = training_job[0]['model_meta_data']
        churn_rates = self.account.churn_info


        # Populate churn report
        churn_report['training_job_data']['training_job_id'] = training_job[0]['_id']
        churn_report['training_job_data']['training_job_date'] = int_to_dt(training_job[0]['updated_at']['$date'], as_str=True)
        churn_report['training_job_data']['model_accuracy'] = meta['eval_score']

        churn_report['churn_data']['current_churn_percentage'] = round(churn_rates['churn_precentage'],1)
        churn_report['churn_data']['churn_direction_indication'] = churn_rates['churn_indicator']
        if churn_rates['churn_difference'] != 0.0:
            churn_report['churn_data']['churn_percentage_difference'] = round(churn_rates['churn_difference'],1)
        if len(churn_rates['features']) > 0:
            churn_report['churn_data']['features_of_importance'] = churn_rates['features']

        return churn_report

