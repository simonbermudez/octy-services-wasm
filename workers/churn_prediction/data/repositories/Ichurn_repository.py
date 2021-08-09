# python imports
from abc import ABC, abstractmethod

class ChurnPredInterface(ABC):

    @abstractmethod
    def get_events(self, account_id : str, profile_ids : list, timeframe : int, event_type : str):
        """
        Parameters
        ----------
        account_id : str
        profile_ids : list
        timeframe : int
        event_type : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError
    
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
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def get_items(self, account_id : str, ids : str):
        """
        Parameters
        ----------
        account_id : str
        ids : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def get_segments(self, account_id : str,  status : str):
        """
        Parameters
        ----------
        account_id : str
        status : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def create_training_job_ref(self, training_job_id : str, account_id : str, meta_data : dict):
        """
        Parameters
        ----------
        training_job_id : str
        account_id : str
        meta_data : dict

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_training_job(self, account_id : str, training_job_id : str, status : str):
        """
        Parameters
        ----------
        training_job_id : str
        account_id : str
        status : str

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def delete_training_job_ref(self, account_id : str, training_job_id : str):
        """
        Parameters
        ----------
        training_job_id : str
        account_id : str

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def start_cloud_training(self, account_id : str, 
                                training_job_id : str, 
                                volume_size : int, 
                                training_resources : list, 
                                bucket_name : str):
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        volume_size : int
            required volume storage for training job.
        training_resources : list
        bucket_name : str

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_cloud_training_status(self, training_job_id : str):
        """
        Parameters
        ----------
        training_job_id : str

        Returns
        ----------
        :rtype: str
        """
        raise NotImplementedError
    
    @abstractmethod
    def update_training_job_ref(self, account_id : str, training_job_id : str, status : str, model_meta : dict): 
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        status : str
        model_meta : dict

        Returns
        ----------
        None
        """
        raise NotImplementedError


    @abstractmethod
    def cache_dataset(self, account_id : str, training_job_id : str, dataset : object): 
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        dataset : str

        Returns
        ----------
        None
        """
        raise NotImplementedError


    @abstractmethod
    def get_cached_dataset(self, account_id : str, training_job_id : str): 
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError


    @abstractmethod
    def delete_cached_dataset(self, account_id : str, training_job_id : str): 
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str

        Returns
        ----------
        None
        """
        raise NotImplementedError