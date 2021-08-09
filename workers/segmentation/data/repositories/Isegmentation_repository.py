# python imports
from abc import ABC, abstractmethod

class SegmentationInterface(ABC):


    @abstractmethod
    def get_profiles(self, account_id : str):
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
    def get_profiles_by_id(self, account_id : str, profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            Octy profile identifier

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    
    @abstractmethod
    def get_events(self, account_id : str, timeframe : int, event_sequence_event : dict):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        timeframe : int
            the number of minutes events must have occurred in, since now
        event_sequence_event : dict
            past segment definition event sequence event object

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def get_segment_definitions(self, account_id : str, type_ : str, segment_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        type_ : str
            segment type. live :: past
        segment_id : str
            segment identifier

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def update_segment_profiles_ids(self, account_id : str, segment_id : str, matching_profile_ids : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_id : str
            Segment identifier
        matching_profile_ids : list
            List of profiles that met this segment criteria on this run

        Returns
        ----------
        None
        """
        raise NotImplementedError