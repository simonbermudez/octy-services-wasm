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
        :rtype: bool, bool, dict
        """
        raise NotImplementedError


    @abstractmethod
    def generate_authorization_token(self, account: str):
        """
        A method used to generate a fat jwt,
        containing account information + authorized tags

        Parameters
        ----------
        account : dict
            Octy account

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