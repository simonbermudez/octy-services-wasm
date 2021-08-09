# python imports
from abc import ABC, abstractmethod

class AlgorithmConfigInterface(ABC):


    @abstractmethod
    def set_algorithm_configs(self, algorithm_config : dict):
        """
            Parameters
            ----------
            algorithm_config : Dict
                Updated algorithm configurations

            Returns
            ----------
            :rtype: : None
        """
        raise NotImplementedError

    @abstractmethod
    def get_items(self, account_id : str):
        """
            Parameters
            ----------
            account_id : str

            Returns
            ----------
            :rtype: : List
        """
        raise NotImplementedError