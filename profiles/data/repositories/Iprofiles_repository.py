# python imports
from abc import ABC, abstractmethod

class ProfilesInterface(ABC):

    @abstractmethod
    def get_profile_count(self, account_id : str):
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
    def get_profile_by_id(self, account_id : str, identifier : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifier : str
            The profile_id or customer_id of the profile that should be returned.

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def get_profiles_by_identifiers(self, account_id : str, identifiers : list, tag_statuses : list, ids : bool, internal : bool):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : str
            A list of identifiers (profile_ids | customer_ids)
        tag_statuses : list
            a list of statuses indicating which segment tags should be returned
        ids : bool
        internal : bool

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_profiles_by_params(self, account_id : str, cursor : int, segments : list, rfm_values : list, churn_prob : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int
            Pagination cursor
        segments : list
            List of segment identifiers
        rfm_values : list
            two integers in a list representing the upper and lower bounds 
            of the desired FRM range to filter profiles by
        churn_prob : str
            label representing the desired churn probability to filter profiles by

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def get_all_profiles(self, 
                        account_id : str, 
                        tag_statuses : list,
                        cursor : int, 
                        ids : bool, 
                        status : str, 
                        limit : int, 
                        internal : bool):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        tag_statuses : list
            a list of statuses indicating which segment tags should be returned
        cursor : int
            pagination cursor
        ids : bool
            Only return profile ids
        status : str
        limit : int
        internal : bool

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError

    @abstractmethod
    def get_merged_profiles(self, account_id : str, identifiers : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : list
            A list of identifiers (profile_ids | customer_ids)

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def create_profiles(self, profiles_batch : list):
        """
        Parameters
        ----------
        profiles_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def update_profiles(self, profiles_batch : list, internal : bool):
        """
        Parameters
        ----------
        profiles_batch : list
            list of profile object dictonaries (valid profile objects)
        internal : bool
            Did update request come from an internal process.

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def delete_profiles(self, profiles_batch : list):
        """
        Parameters
        ----------
        profiles_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError
    
    @abstractmethod
    def update_delete_segment_tags(self, account_id : str, segment_ids : list, action : str):
        """
        Parameters
        ----------
        account_id : str
            octy account id
        segment_ids : list
        action : str
            update or delete

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def create_segment_tags(self, account_id : str, profile_id : str, segment_tags : list):
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to create

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def update_segment_tags(self, account_id : str, profile_id : str, segment_tags : list):
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to create

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def delete_segment_tags(self, account_id : str, profile_id : str, segment_tags : list):
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to create

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def set_profile_key_type(self, account_id : str, profile_key_type : dict):
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_key_type : dict
            ex : {'key' : 'age', 'type_' : '<class 'int'>'}

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def get_profile_key_types(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            octy account id

        Returns
        ----------
        list
        """
        raise NotImplementedError