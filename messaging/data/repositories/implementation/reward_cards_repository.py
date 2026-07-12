# module imports
from data.repositories.Ireward_cards_repository import RewardCardsInterface
from utils.utils import *
from api.routers.error_handlers import *


# python imports
from typing import *
import json
from datetime import datetime as dt
import time
import math

# external imports



class _RybbonRewardCardsRepository(RewardCardsInterface):
    """
        _RybbonRewardCardsRepository
        Handles:
        - Retrieving templates
        - Creating templates
        - Updating templates
        - Deleting templates

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def auth(self) -> str: 
        url = f"{Config['RYBBON_AUTH_URL']}"

        session = requests_retry_session()

        t0 = time.time()
        try:
            response = session.post(
                url,
                data={
                    'grant_type' : "client_credentials",
                    'client_id' : Config['RYBBON_CLIENT_ID']
                },
                timeout=60,
                headers={
                    'Partner-Id' : Config['RYBBON_PARTNER_ID'],
                    'Content-Type': 'application/x-www-form-urlencoded'
                }
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')
            
        body = json.loads(response.text)
        return body['access_token']

    async def get_campaigns(self, auth_token: str) -> list:
        """
        Parameters
        ----------
        auth_token: str
            Rybbon authorization token

        Returns
        ----------
        :rtype: list
        """
        url = f"{Config['RYBBON_CAMPAIGNS_URL']}"
        campaigns = list()
        exhausted_campaigns = False
        cursor : int = 0

        session = requests_retry_session()
        while not exhausted_campaigns:
            t0 = time.time()
            try:
                response = session.get(
                    f"{url}?limit=1000&filterByStatus=open?start={cursor}",
                    timeout=60,
                    headers={
                        'Partner-Id' : Config['RYBBON_PARTNER_ID'],
                        'Authorization' : f'Bearer {auth_token}',
                        'Content-Type': 'application/json'
                    }
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')

            if 200 <= response.status_code < 500:
                exhausted_campaigns = True
                
            body = json.loads(response.text)
            campaigns.extend(body['result']['campaign'])
            cursor += 1000

        return campaigns

    async def claim_rewards(self, auth_token: str, claim_groups : list) -> list:
        """
        Parameters
        ----------
        auth_token: str
            Rybbon authorization token
        claim_groups : list
            List containing the required parameters to claim reward cards

        Returns
        ----------
        :rtype: list
        """
        def request(post_body):
            url = f"{Config['RYBBON_REWARD_CLAIM_URL']}"
            session = requests_retry_session()

            t0 = time.time()
            try:
                response = session.post(
                    url,
                    timeout=60,
                    data=json.dumps(post_body),
                    headers={
                        'Partner-Id' : Config['RYBBON_PARTNER_ID'],
                        'Authorization' : f'Bearer {auth_token}',
                        'Content-Type': 'application/json'
                    }
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')
 
            body = json.loads(response.text)
            if body['success'] != True or body['rewardAvailable'] != True:
                return []
            
            return body['result']

        def _filter_valid_claims(claims : list) -> list:
            return list(filter(lambda x : x['active'] == True and x['exceeded'] == False, claims))

        rewards = list()
        for claims in claim_groups:

            rybbon_campaign_key = claims[0]['campaignKey']

            # Filter out exceeded or no active claims
            valid_claims = _filter_valid_claims(claims)
            valid_claims = [{k: v for k, v in d.items() if k not in ['active', 'exceeded', 'campaignKey']} for d in valid_claims]
            if len(valid_claims) < 1:
                continue

            # chunk claims
            num_claims_req = 1
            if len(valid_claims) > 100:
                num_claims_req = math.ceil(len(valid_claims) / 100)
            chunk_counter = 0
            for _ in range(num_claims_req):

                # create full request object and pass to request
                post_body = {
                    "campaignKey" : rybbon_campaign_key,
                    "rewardClaims" : valid_claims[chunk_counter:chunk_counter+100]
                }

                # get response and append to parent rewards to return
                rewards.extend(request(post_body))
                chunk_counter += 100

        return rewards

rybbonRewardCardsRepository = _RybbonRewardCardsRepository()