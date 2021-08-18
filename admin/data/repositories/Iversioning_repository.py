# python imports
from abc import ABC, abstractmethod

class VersioningInterface(ABC):

    @abstractmethod
    def cache_version_data(self, data : dict, repository_name : str):
        """
        Parameters
        ----------
        data : dict
            The version data that will be cached
        repository_name : str
            The name of the repository version info is being cached for

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError

    @abstractmethod
    def get_cached_version_data(self, key : str):
        """
            Parameters
            ----------
            key : str
                Key to get version data for

            Returns
            ----------
            :rtype: object
        """
        raise NotImplementedError