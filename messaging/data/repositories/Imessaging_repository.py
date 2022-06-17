# python imports
from abc import ABC, abstractmethod

class MessagingContentInterface(ABC):

    @abstractmethod
    def get_item_recommendations(self, account_id : str, profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    
    @abstractmethod
    def get_items(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_currency_rates(self):
        """
        Parameters
        ----------
        None

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError