# python imports
from abc import ABC, abstractmethod

class BillingInterface(ABC):

    @abstractmethod
    def create_billable_units_ref(self, untis : list):
        """
        Parameters
        ----------
        untis : list
            List of billable unit objects

        Returns
        ----------
        :rtype: None
        """
        raise NotImplementedError

    @abstractmethod
    def filter_billable_units(self, filters : dict, cursor : int):
        """
        Parameters
        ----------
        filters : dict
            Specific filter parameters
        cursor : int
            Pagination cursor

        Returns
        ----------
        :rtype: list, int
        """
        raise NotImplementedError