# python imports
from abc import ABC, abstractmethod

class NotificationsInterface(ABC):

    @abstractmethod
    def email(self, payload: dict):
        """
        Parameters
        ----------
        payload : dict
            Dictionary object containing message content and meta data

        Returns
        ----------
        :rtype: bool
        """
        raise NotImplementedError

    @abstractmethod
    def webhook(self, payload: dict):
        """
        Parameters
        ----------
        payload : dict
            Dictionary object containing message content and meta data

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError