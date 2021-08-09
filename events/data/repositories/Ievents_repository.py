# python imports
from abc import ABC, abstractmethod
from typing import Union

class EventsInterface(ABC):

    @abstractmethod
    def create_event(self, account_id : str, event : dict):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event : dict

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError

    @abstractmethod
    def batch_create_events(self, account_id : str, events_batch : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        events_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    
    @abstractmethod
    def get_events_meta(self, account_id : str, event_type_list : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_list : list

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def get_events(self, account_id : str, 
                    timeframe : int, 
                    cursor : int, 
                    event_sequence_event : dict, 
                    profile_ids : list, 
                    event_type : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        timeframe : int
            the number of minutes events must have occurred in, since now
        cursor : int
            pagination cursor
        event_sequence_event : dict
            past segment definition event sequence event object
        profile_ids : list
            A list of octy profile identifiers
        event_type : str

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def delete_profile_events(self, account_id : str, profile_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_id : str
            octy profile identifier

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def get_profile_ids(self, account_id : str, profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            list of profile ids to verfiy existence of

        Returns
        ----------
        :rtype: list, list
        """

    @abstractmethod
    def get_live_segment_definitions(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account identifier

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError
    