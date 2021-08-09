# module imports
from data.repositories.Isegmentation_repository import SegmentationInterface
from data.models.db_schemas import tbl_segments
from utils.utils import *
from config import Config

# python imports
from typing import *
import json
from datetime import datetime as dt
import time

# external imports
from mongoengine.queryset.visitor import Q


class _SegmentationRepository(SegmentationInterface):
    """
        _SegmentationRepository
        Handles:
        - Get Segment definitions
        - Get profiles + id
        - Get events

        ...

        Attributes
        ----------
        ...
    """
    def __init__(self): pass

    async def get_profiles(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        profiles : list
        """
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids=false"
        profiles = []
        payload = {
            'account_id': account_id,
            'profiles' : [], 
            'tag_statuses' : ["active", "pending", "inactive"],
            'get_all': True
        }
        exhausted_profiles = False

        cursor : int = 0
        session = requests_retry_session()
        while not exhausted_profiles:
            t0 = time.time()
            try:
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=5
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')


            if response.status_code != 200:
                exhausted_profiles = True
                continue

            body = json.loads(response.text)
            for profile in body['profiles']:
                profiles.append(
                    profile
                )
            cursor +=body['request_meta']['count']

        return profiles

    async def get_profiles_by_id(self, account_id : str, profile_ids : list) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            Octy profile identifier

        Returns
        ----------
        profiles: list
        """
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids=false"
        profiles = []

        #chunk requests based on count of profile_ids
        for pid_chunk in list(chunks(profile_ids, 2000)):
            payload = {
                'account_id': account_id,
                'profiles' : pid_chunk, 
                'tag_statuses' : ["active", "pending", "inactive"],
                'get_all': False
            }
            session = requests_retry_session()
            t0 = time.time()

            try:
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(0)},
                    timeout=5
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')

            if response.status_code != 200:
                return profiles

            body = json.loads(response.text)
            for profile in body['profiles']:
                profiles.append(
                    profile
                )

        return profiles

    async def get_events(self, account_id : str, timeframe : int, event_sequence_event : dict, profile_ids : list = None) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        timeframe : int
            the number of minutes events must have occurred in, since now
        event_sequence_event : dict
            past segment definition event sequence event object
        Returns
        ----------
        events : list
        """
        url = f"{Config['EVENT_SERVICE_CLUSTER_IP']}/v1/internal/events"
        events = []
        exhausted_events = False
        cursor : int = 0
        session = requests_retry_session()

        if profile_ids:
            payload = {
                'event_sequence_event' : event_sequence_event,
                'timeframe' : timeframe,
                'account_id' : account_id
            }
        else:
            payload = {
                'event_sequence_event' : event_sequence_event,
                'timeframe' : timeframe,
                'profile_ids' : profile_ids,
                'account_id' : account_id
            }


        while not exhausted_events:
            t0 = time.time()
            try:
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=5
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
            finally:
                t1 = time.time()
                print('Took', t1 - t0, 'seconds')

            if response.status_code != 200:
                exhausted_events = True
                continue

            body = json.loads(response.text)
            for event in body['events']:
                events.append(
                    event
                )
            cursor += body['request_meta']['count']

        return events

    async def get_segment_definitions(self, account_id : str, type_ : str = None, segment_id : str = None) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        type_ : str
            segment type. live :: past
        segment_id : str
            segment identifier

        Returns
        ----------
        found_segments : list
        """
        found_segments =[]
        
        if segment_id:
            segments = tbl_segments.objects((Q(segment_id__exact=segment_id) & Q(account_id__exact=account_id) & Q(status__exact='active')))
        else:
            segments = tbl_segments.objects((Q(segment_type__exact=type_) & Q(account_id__exact=account_id) & Q(status__exact='active')))

        for segment in segments:
            segment_dict = json.loads(segment.to_json())
            segment_dict['segment_id'] = segment_dict['_id']
            for event in segment_dict['event_sequence']:
                try:
                    event['event_properties']
                except KeyError:
                    event['event_properties'] = None
            found_segments.append(segment_dict)

        return found_segments

    async def update_segment_profiles_ids(self, account_id : str, segment_id : str, matching_profile_ids : list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_id : str
            Segment identifier
        matching_profile_ids : list
            List of profiles that met this segment criteria on this run

        Returns
        ----------
        :rtype: None
        """

        bulk_operation = tbl_segments._get_collection().initialize_unordered_bulk_op()

        bulk_operation.find({
                '$and' : [
                    {"account_id" : { "$eq" : account_id} },
                    {"_id" : { "$eq" : segment_id} }
                ]
            }).update(
            
                    {'$set': {"profile_ids" : matching_profile_ids}}
                
            )
        bulk_operation.execute()






segmentationRepository = _SegmentationRepository()