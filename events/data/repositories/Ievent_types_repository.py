# get by id, get all, get count, create, delete
# python imports
from abc import ABC, abstractmethod

class EventTypesInterface(ABC):

    @abstractmethod
    def get_event_types_count(self, account_id : str):
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
    def get_event_type_by_id(self, account_id : str, event_type_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_id : str
            The event_type_id of the event type that should be returned.

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError
    
    
    @abstractmethod
    def get_event_type_by_name(self, account_id : str, event_type : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type : str
            The event_type name of the event type that should be returned.

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def get_event_types_by_name(self, account_id : str, event_type_names : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_names : list
            list of event_type names

        Returns
        ----------
        :rtype: list, list
        """

    @abstractmethod
    def get_all_event_types(self, account_id : str, cursor : int):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int

        Returns
        ----------
        :rtype: dict, int
        """
        raise NotImplementedError

    @abstractmethod
    def create_event_types(self, event_types : list):
        """
        Parameters
        ----------
        event_types : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def delete_event_types(self, event_types_batch : list):
        """
        Parameters
        ----------
        event_types_batch : list[str]

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError