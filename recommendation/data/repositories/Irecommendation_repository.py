# python imports
from abc import ABC, abstractmethod

class RecommendationsInterface(ABC):

    @abstractmethod
    def get_latest_hp_tuning_job(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def get_cached_recommendations(self,
                                account_id : str,
                                training_job_id : str,
                                profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        profile_ids : list

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError
