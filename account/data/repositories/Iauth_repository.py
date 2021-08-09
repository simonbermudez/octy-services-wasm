# python imports
from abc import ABC, abstractmethod

class AuthInterface(ABC):

    @abstractmethod
    def verify_account_keys(self, pk: str, sk: str):
        """
        Parameters
        ----------
        pk : str
            Octy public key
        sk : str
            Octy secret key

        Returns
        ----------
        :rtype: bool, bool
        """
        raise NotImplementedError


    @abstractmethod
    def auth_token(self, pk: str):
        """
        Parameters
        ----------
        pk : str
            Octy public key

        Returns
        ----------
        :rtype: str
        """
        raise NotImplementedError

    @abstractmethod
    def log_failed_auth(self, failed_attempt : object):
        """
        Parameters
        ----------
        failed_attempt : Dict
            Dict containing required data to log a failed authentication attempt

        Returns
        ----------
        :rtype: object
        """
        raise NotImplementedError