# python imports
from abc import ABC, abstractmethod

class ItemsInterface(ABC):

    @abstractmethod
    def get_item_count(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: int
        """
        raise NotImplementedError


    @abstractmethod
    def get_item_by_id(self, item_id : str, account_id : str):
        """
        Parameters
        ----------
        item_id : str
            The item_id of the item that should be returned.
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_items(self, 
                account_id : str,
                cursor : int,
                ids : bool,
                status : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int
            Pagination cursor
        ids : bool
            Only get item id(s)
        status : str

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def create_items(self, items_batch):
        """
        Parameters
        ----------
        items_batch : list
            list of item object dictonaries (valid item objects)

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def update_items(self, items_batch : list, account_id : str):
        """
        Parameters
        ----------
        items : UpdateItems
            UpdateItems request model instance
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def delete_items(self, items_batch : list, account : object):
        """
        Parameters
        ----------
        items_batch : List
            list of item object dictonaries (valid item objects)
        account : Octy account

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError