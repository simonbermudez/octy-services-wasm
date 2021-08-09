# python imports
from abc import ABC, abstractmethod

class BucketInterface(ABC):

    @abstractmethod
    def create_bucket(self, bucket_name: str):
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        Returns
        ----------
        :rtype: bool
        """
        raise NotImplementedError

    @abstractmethod
    def bucket_configuration(self, bucket_name: str):
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        Returns
        ----------
        :rtype: bool
        """
        raise NotImplementedError

    @abstractmethod
    def create_directory(self, bucket_name: str, directory_path : str):
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        directory_path : str
            Path where the new directory should be created

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError