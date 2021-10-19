# python imports
from abc import ABC, abstractmethod

class ProfilesIdenInterface(ABC):

    @abstractmethod
    def get_profiles(self, account_id : str, status : str, ids : str):
        """
        Parameters
        ----------
        account_id : str
        status : str
        ids : str

        Returns
        ----------
        profiles : list
        """
        raise NotImplementedError

    @abstractmethod
    def create_merged_profiles_ref(self, merged_profiles : list):
        """
        Parameters
        ----------
        merged_profiles : list

        Returns
        ----------
        :rtype: None
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