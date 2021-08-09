# module imports
from data.repositories.implementation.recommendation_repository import recommendationsRepository
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
import json

# external imports
from fastapi import Request


class RecommendationsService():
    """
        RecommendationsService
        Handles:
        - Get Recommendations
        ...

        Attributes
        ----------
        account : Octy account
        account_id : str
    """
    def __init__(self, account : Account, account_id : str): 
        self.account = account
        self.account_id = account_id if account_id != None else account.account_id
    
    async def _filter_recommendations(self, profile_id, recommendations)-> list:
        return list(filter(lambda x : x['profile_id'] == profile_id, recommendations))

    async def get_recommendations(self, profile_ids : list) -> Union[list, dict]:
        """
        Parameters
        ----------
        profile_ids : list

        Returns
        ----------
        Item recommendations, training_job meta : Union[list, dict]
        """
        training_job = await recommendationsRepository.get_latest_training_job(account_id=self.account_id)
        if not training_job:
            raise OctyException(400,'An error occurred when getting item recommendations', 
                [{'message' : 'No recommendations training jobs have been completed. Recommendations training jobs are automatically run every 24 hours', 
                'extended_help': Config['RECOMENDATIONS_EXTENDED_HELP']}])
        
        recommendations_cache = await recommendationsRepository.get_cached_recommendations(account_id=self.account_id,
                                                                                    training_job_id=training_job[0]['_id'],
                                                                                    profile_ids=profile_ids)
        recommendations = list()
        for profile_id in profile_ids:
            profile_recommendations = await self._filter_recommendations(profile_id=profile_id,
                                                                        recommendations=recommendations_cache)
            if len(profile_recommendations) < 1:
                recommendations.append(
                        {
                            'profile_id' : profile_id,
                            'recommendations' : [],
                            'error' : 'Profile does not exist or Insufficient number of items available to make recommendations for this profile, if \'recommend_interacted_items\' set to \'false\' in your recommendations algorithm configurations, try setting it to \'true\' if this frequently occurs.'
                        })
                continue
            
            recommendations.append(
                {
                        'profile_id' : profile_id,
                        'recommendations' : profile_recommendations[0]['recommendations'][:10],
                        'error' : None
                }
                
            )
        meta = training_job[0]['model_meta_data']
        meta['training_job_id'] = training_job[0]['_id']
        meta['model_created_at'] = int_to_dt(training_job[0]['updated_at']['$date'], as_str=True)
        return recommendations, meta
