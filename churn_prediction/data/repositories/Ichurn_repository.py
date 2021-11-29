# python imports
from abc import ABC, abstractmethod

class ChurnPredInterface(ABC):
    
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