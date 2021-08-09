# python imports
from abc import ABC, abstractmethod

class AccountConfigInterface(ABC):

    @abstractmethod
    def set_account_configs(self, account_config : object):
        """
            Parameters
            ----------
            account_config : Dict
                Updated account configurations

            Returns
            ----------
            :rtype: : None
        """
        raise NotImplementedError