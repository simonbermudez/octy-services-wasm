# python imports
from abc import ABC, abstractmethod

class TemplatesInterface(ABC):

    @abstractmethod
    def get_all_templates(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: list
        """

    @abstractmethod
    def get_template_count(self, account_id : str):
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: int
        """

    @abstractmethod
    def get_templates(self, account_id : str, _id : str, cursor : int):
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        _id : str
        cursor : int

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError
    
    @abstractmethod
    def create_templates(self, templates_batch : list):
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def update_templates(self, templates_batch : list):
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError

    @abstractmethod
    def delete_templates(self, templates_batch : list):
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        :rtype: list, list
        """
        raise NotImplementedError