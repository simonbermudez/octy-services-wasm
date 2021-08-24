# python imports
from abc import ABC, abstractmethod

class RecommendationsInterface(ABC):

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
    def get_profiles(self, account_id : str, ids : str):
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
    def get_items(self, account_id : str, ids : str, status : str):
        """
        Parameters
        ----------
        account_id : str
        ids : str
        status : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def get_segments(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def create_hparam_tuning_job_ref(self, items_df : object, profiles_df : object, hyperparam_tuning_job_id : str, account_id : str, meta_data : dict):
        """
        Parameters
        ----------
        items_df : object
        profiles_df : object
        hyperparam_tuning_job_id : str
        account_id : str
        meta_data : dict

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def get_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str, status : str):
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str
        account_id : str
        status : str

        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def get_parent_hparam_tuning_job_ref(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str


        Returns
        ----------
        :rtype: dict
        """
        raise NotImplementedError

    @abstractmethod
    def update_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str, best_model_training_job_id :str, status : str, model_meta : dict): 
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str
        best_model_training_job_id :str
        status : str
        model_meta : dict

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def delete_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str):
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str
        account_id : str

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def start_hparam_tuning_job(self, 
                            account_id : str, 
                            hyperparam_tuning_job_id : str,
                            parent_hyperparam_tuning_job_id : str or None,
                            volume_size : int, 
                            training_resources : list, 
                            bucket_name : str):
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str
        parent_hyperparam_tuning_job_id : str | None
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
    def get_hparam_tuning_job_status(self, hyperparam_tuning_job_id : str):
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str

        Returns
        ----------
        status : str
        """
        raise NotImplementedError

    @abstractmethod
    def get_best_training_job(self, hyperparam_tuning_job_id : str):
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str

        Returns
        ----------
        best_training_job : dict
        """
        raise NotImplementedError
    
    @abstractmethod
    def cache_item_recommendations(self, account_id : str, training_job_id : str, predictions : list):
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        predictions : list

        Returns
        ----------
        None
        """
        raise NotImplementedError