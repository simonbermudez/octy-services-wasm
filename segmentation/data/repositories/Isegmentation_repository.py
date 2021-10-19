# python imports
from abc import ABC, abstractmethod

class SegmentationInterface(ABC):


    @abstractmethod
    def get_segment_count(self, account_id : str):
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
    def get_segment_by_id_name(self, segment_id_name : str, account_id : str):
        """
        Parameters
        ----------
        segment_id_name : str
            Segment identifier
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError
        
    @abstractmethod
    def get_segment_by_attr(self, account_id : str, segment : dict):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment : dict
            Pagination cursor

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def get_past_segments_by_profile_ids(self, account_id : str, profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_segments(self, 
                    account_id : str, 
                    segment_type : str, 
                    status : str, 
                    cursor : int, 
                    internal : bool):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_type : str
            live, past or all
        status : str
            the status to filter results by
        cursor : int
            Pagination cursor
        internal : bool

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def create_segment(self, segment : object):
        """
        Parameters
        ----------
        segment : object

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def update_segment_status(self, account_id : str, segment_ids : list, status : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers
        status : str
            the status to filter results by

        Returns
        ----------
        :rtype: List, List
        """
        raise NotImplementedError

    @abstractmethod
    def update_past_segment_profile_ids(self, account_id : str, segment_id : str, profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_id : str
            segment identifier
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError

    @abstractmethod
    def delete_segments(self, account_id : str, segment_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers

        Returns
        ----------
        None
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
        raise NotImplementedError
    