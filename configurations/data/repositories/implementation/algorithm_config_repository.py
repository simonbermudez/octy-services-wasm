# module imports
from data.repositories.Ialgorithm_config_repository import AlgorithmConfigInterface
from config import Config
from utils.utils import *
from octy_rabbitmq.amqp_publisher import amqpPublisher

# python imports
from typing import *
import json
import requests
from requests.adapters import HTTPAdapter
from requests.packages.urllib3.util.retry import Retry
import time

# external imports


class _AlgorithmConfigRepository(AlgorithmConfigInterface):
    """
        _AlgorithmConfigRepository
        Handles:
        - Updating algorithm configurations

        ...

        Attributes
        ----------
        pk : str
            Octy generated public key
    """
    def __init__(self): pass

    async def set_algorithm_configs(self, algorithm_config : dict) -> None:
        """
            A method used to get either all algorithm configurations or
            specified algorithm configurations.

            Parameters
            ----------
            algorithm_config : object
                Updated algorithm configurations

            Returns
            ----------
            None
        """

        await amqpPublisher.send_message(routing_key='algo.configs.cmd.update',
        payload={
            "account_id" : algorithm_config.account_id,
            "algorithm_configurations" : {
                 "algorithm_name" : algorithm_config.algorithm_name,
                 "config_json" : algorithm_config.configurations.dict()
            }
           
        })
    
    async def get_items(self, account_id : str) -> List:
        """
            Parameters
            ----------
            account_id : str
        
            Returns
            ----------
            item_ids : List
        """
        print(f"Getting items for account: {account_id}")
        #?ids=true only return item ids
        url = f"{Config['ITEM_SERVICE_CLUSTER_IP']}/v1/internal/items?account_id={account_id}&ids=true&status=all"
        item_ids = []
        exhausted_items = False

        cursor : int = 0
        session = requests_retry_session()
        while not exhausted_items:
            t0 = time.time()
            try:
                response = session.get(
                    url,
                    headers={'cursor': str(cursor)},
                    timeout=60
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')


            if response.status_code != 200:
                exhausted_items = True
                continue

            body = json.loads(response.text)
            for item in body['items']:
                item_ids.append(
                    item['item_id']
                )
            cursor +=body['request_meta']['count']

        return item_ids




algorithmConfigRepository = _AlgorithmConfigRepository()