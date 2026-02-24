# module imports
from data.repositories.Imessaging_repository import MessagingContentInterface
from data.models.db_schemas import tbl_currency_rates
from utils.utils import *
from api.routers.error_handlers import *
from config import Config

# python imports
from typing import *
import json
import time

# external imports


class _MessagingContentRepository(MessagingContentInterface):
    """
        _MessagingContentRepository
        Handles:
        - Retrieving item recommendations for message content
        - Retrieving items
        - Retrieving currency rates

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def get_item_recommendations(self, account_id : str, profile_ids : list) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list

        Returns
        ----------
        recommendations : list
        """
        url = f"{Config['REC_SERVICE_CLUSTER_IP']}/v1/internal/recommendations"
        session = requests_retry_session()

        payload = {
            'account_id' : account_id,
            'profile_ids' : profile_ids
        }

        t0 = time.time()
        try:
            response = session.post(
                url,
                data=json.dumps(payload),
                timeout=60,
                headers={'Content-Type': 'application/json'}
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')

        if response.status_code == 400:
            return []
            
        body = json.loads(response.text)
        return body['recommendations']

    async def get_items(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        :rtype: list
        """
        url = f"{Config['ITEM_SERVICE_CLUSTER_IP']}/v1/internal/items?account_id={account_id}&ids=false&status=active"
        items = []
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
                items.append(
                    item
                )
            cursor +=body['request_meta']['count']

        return items

    async def get_currency_rates(self) -> dict:
        """
        Parameters
        ----------
        None

        Returns
        ----------
        :rtype: dict
        """
        rates = tbl_currency_rates.objects.order_by('-created_at').first().to_mongo().to_dict()
        return rates['rates']

    # delete messaging data to do with account_id
    # TODO : check where the messaging content is stored
    async def delete_account_messaging_data(self, account_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        True if account was deleted successfully, False otherwise : bool
        """
        # Delete messaging content
        res = messagingContentRepository.delete_messaging_content(account_id)
        if res is False:
            raise Exception(500, 'Messaging content could not be deleted.')

        return True
    

messagingContentRepository = _MessagingContentRepository()