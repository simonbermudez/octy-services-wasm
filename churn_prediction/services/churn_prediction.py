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
    def __init__(self, account : Account, account_id : str = None): 
        self.account = account
        self.account_id = account_id if account_id is not None else account.account_id

    async def generate_churn_report(self) -> dict:
        """
        Parameters
        ----------

        Returns
        ----------
        churn_report : dict
        """
        hp_tuning_job = await churnPredictionRepository.get_latest_hp_tuning_job(account_id=self.account.account_id)
        if not hp_tuning_job:
            raise OctyException(400,'An error occurred when generating this churn prediction report.', 
                [{'error_message' : 'No churn prediction training jobs have been completed. Churn prediction training jobs are automatically run every 24 hours', 
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

        meta = hp_tuning_job[0]['best_model_meta_data']
        churn_rates = self.account.churn_info


        # Populate churn report
        churn_report['training_job_data']['training_job_id'] = hp_tuning_job[0]['best_model_training_job_id']
        churn_report['training_job_data']['training_job_date'] = int_to_dt(hp_tuning_job[0]['updated_at']['$date'], as_str=True)
        churn_report['training_job_data']['model_accuracy'] = meta['eval_score']

        churn_report['churn_data']['current_churn_percentage'] = round(churn_rates['churn_percentage'],1)
        churn_report['churn_data']['churn_direction_indication'] = churn_rates['churn_indicator']
        if churn_rates['churn_difference'] != 0.0:
            churn_report['churn_data']['churn_percentage_difference'] = round(churn_rates['churn_difference'],1)
        if len(churn_rates['features']) > 0:
            churn_report['churn_data']['features_of_importance'] = churn_rates['features']

        return churn_report


    # delete all churn predictions data associated with an account
    async def delete_account_churn_predictions_internal(self) -> bool:
        """
        Parameters
        ----------

        Returns
        ----------
        bool
        """
        res = await churnPredictionRepository.delete_account_churn_predictions(account_id=self.account_id)
        return res