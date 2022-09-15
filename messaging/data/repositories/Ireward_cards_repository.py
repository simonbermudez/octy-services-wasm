# python imports
from abc import ABC, abstractmethod

class RewardCardsInterface(ABC):

    @abstractmethod
    def get_campaigns(self, auth_token: str):
        """
        Parameters
        ----------
        auth_token: str
            Rybbon authorization token

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    
    @abstractmethod
    def claim_rewards(self, auth_token: str, claim_groups : list):
        """
        Parameters
        ----------
        auth_token: str
            Rybbon authorization token
        claim_groups : list
            List containing the required parameters to claim reward cards

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError