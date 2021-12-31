# python imports
from abc import ABC, abstractmethod

class AccountInterface(ABC):

    @abstractmethod
    def create_account(self, account, bucket : str):
        """
        Parameters
        ----------
        account : CreateAccount
            CreateAccount request model instance
        bucket : str
            Bucket unique indentifier 

        Returns
        ----------
        :rtype: object, str
        """
        raise NotImplementedError

    @abstractmethod
    def get_account(self, pk : str, dict : bool):
        """
            Parameters
            ----------
            pk : str
                Octy generated account public key

            dict : bool
                Whether the return account as dict object

            Returns
            ----------
            :rtype: object
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_accounts(self, account_ids : list, cursor : int):
        """
            Parameters
            ----------
            account_ids : list
            cursor : int

            Returns
            ----------
            :rtype: list
            :rtype: int
        """
        raise NotImplementedError

    @abstractmethod
    def update_account(self, account, action : str):
        """
            Parameters
            ----------
            account : UpdateAccount
                UpdateAccount request model instance
            action : str
                Define which parts of account should be updated
            Returns
            ----------
            None
        """
        raise NotImplementedError

    @abstractmethod
    def delete_account(self, account_id : str):
        """
            Parameters
            ----------
            account_id : str
                Octy generated unique account identifier

            Returns
            ----------
            :rtype: None
        """
        raise NotImplementedError

    @abstractmethod
    def update_account_cache(self, account : dict):
        """
            Parameters
            ----------
            account : dict
                Octy account

            Returns
            ----------
            :rtype: None
        """
        raise NotImplementedError