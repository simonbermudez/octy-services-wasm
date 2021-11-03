# python imports
from abc import ABC, abstractmethod

class OctyJobsInterface(ABC):

    @abstractmethod
    def create_octy_job(self, account_ids : list, octy_job : dict):
        """
        Parameters
        ----------
        account_id : list
            Octy account id
        octy_job : dict

        Returns
        ----------
        None
        """
        raise NotImplementedError
    
    @abstractmethod
    def update_octy_job(self, octy_job_updates : list):
        """
        Parameters
        ----------
        octy_job_updates : list

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def delete_octy_jobs(self, account_id : str, identifiers : list):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : list

        Returns
        ----------
        None
        """
        raise NotImplementedError

    @abstractmethod
    def get_octy_jobs(self, cursor : int) -> list:
        """
        Parameters
        ----------
        cursor : int

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def get_pending_job_accounts(self, account_ids : list):
        """
        Parameters
        ----------
        account_ids : list

        Returns
        ----------
        :rtype: list
        """
        raise NotImplementedError

    @abstractmethod
    def claim_pending_job(self, account_id : str, octy_job_id : str, pod_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str
        octy_job_id : str
        pod_id : str

        Returns
        ----------
        :rtype: bool
        """
        raise NotImplementedError