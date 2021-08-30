# module imports
from data.repositories.Ievents_repository import EventsInterface
from utils.utils import *
from api.routers.error_handlers import *
from data.models.db_schemas import tbl_event_instances

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import time

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps


class _EventsRepository(EventsInterface):
    """
        _EventsRepository
        Handles:
        - Creating events (single and batch)
        - Get events descending
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def create_event(self, account_id : str, event : dict) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event : dict

        Returns
        ----------
        None
        """
        db_event = tbl_event_instances(
            event_id=event['event_id'],
            profile_id=event['profile_id'],
            account_id=account_id,
            event_type_id=event['event_type_id'],
            event_type=event['event_type'],
            event_properties=event['event_properties']
        )
        db_event.save()

    async def batch_create_events(self, account_id : str, events_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        events_batch : list

        Returns
        ----------
        created_events, failed_to_create : list, list
        """
        event_instances = []
        event_ids = []
        for event in events_batch:
            event_instances.append(
                tbl_event_instances(
                    event_id=event['event_id'],
                    profile_id=event['profile_id'],
                    account_id=account_id,
                    event_type_id=event['event_type_id'],
                    event_type=event['event_type'],
                    event_properties=event['event_properties'],
                    created_at=event['created_at'])
            )
            event_ids.append(event['event_id'])

        #BULK WRITE OPERATION
        invalid=[]
        bulk_operation = tbl_event_instances._get_collection().initialize_unordered_bulk_op()
        for event in event_instances:
            bulk_operation.insert(event.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                invalid.append(err['op'].to_dict()['_id'])

        valid = list(set(event_ids) - set(invalid))

        failed_to_create=[]
        for in_ in invalid:
            failed_to_create.append(
                {
                    'event_id': in_,
                    'error_message' : f'Another event exists with provided event_id : {in_}'
                }
            )
        created_events=[]
        for v in valid:
            event=next((d for i,d in enumerate(events_batch) if v == d['event_id']),None)
            if event:
                event.pop('account_id', None)
                created_events.append(event)
        
        return created_events, failed_to_create

    async def get_events_meta(self, account_id : str, event_type_list : list) -> Union[list, int]:
        """
        Get latest event instance of EACH provided event type in events to determine each event property required data type.
        Also get current count of total events associated with an account, to ensure account is not surpassing creation limit

        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_list : list[str]

        Returns
        ----------
        event_types : list[dict]
        event_count : int
        """
        event_types = []
        queries_idxs = []
        current_count = tbl_event_instances.objects(account_id__exact=account_id).count()
        
        #get latest events that the event type is in event_type_list
        queries = [{
            '$facet' : {

            }
        }]
        for idx, et in enumerate(event_type_list): 

            queries[0]['$facet']['query'+str(idx)] = [
                {'$match' : 
                    { '$and' : [ {"event_type" : { "$eq" : et}}, {"account_id" : { "$eq" : account_id}} ] }
                },
                { '$sort' : { 'created_at' : -1 } },
                { '$limit' : 1 }
            ]
            queries_idxs.append('query'+str(idx))

        results = tbl_event_instances._get_collection().aggregate(queries)
        try:
            results_dicts = json.loads(dumps(results))[0]
        except KeyError:
            return event_types, current_count

        for q in queries_idxs:
            try:
                event_types.append(
                    {
                        'event_type' : results_dicts[q][0]['event_type'],
                        'event_properties' : results_dicts[q][0]['event_properties']
                    }
                )
            except IndexError:
                continue

        return event_types, current_count

    async def get_events(self, account_id : str, 
                        timeframe : int, 
                        cursor : int, 
                        event_sequence_event : dict = None, 
                        profile_ids : list = None, 
                        event_type : str = None) -> Union[list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        timeframe : int
            the number of minutes events must have occurred in, since now
        cursor : int
            pagination cursor
        event_sequence_event : dict
            past segment definition event sequence event object
        profile_ids : list
            A list of octy profile identifiers
        event_type : str

        Returns
        ----------
        events : list
        total : int
        """
        
        datetime_timeframe = dt.now() - td(minutes=timeframe+1) # NOTE: Add one additional minute
        if event_sequence_event:
            raw_events = []
            event_property_keys = []
            query = {

                    '$and' : [
                        {'account_id' : { '$eq' : account_id}},
                        {'created_at' : { '$gt' : datetime_timeframe}},
                        {'event_type' : { '$eq' : event_sequence_event['event_type']}}
                    ]
                
                }
            if event_sequence_event['event_properties'] != None:
                for k, v in event_sequence_event['event_properties'].items():
                    if k not in event_property_keys:
                        query['$and'].append(
                            {
                                '$and' : [
                                    {f'event_properties.{k}': {"$exists": True}},
                                    {f'event_properties.{k}': {"$eq": v}}
                                ]
                            }
                        )
                        event_property_keys.append(k)

            results_cursor = tbl_event_instances._get_collection().find(query).skip(cursor).limit(3000)
            total = tbl_event_instances._get_collection().find(query).count()
            raw_events.extend(json.loads(dumps(list(results_cursor), indent = 2)))
            found_events=[]
            for event in raw_events:
                event_dict = event
                event_dict['event_id'] = event_dict['_id']
                event_dict.pop('_id', None)
                event_dict['created_at'] = int_to_dt(event_dict['created_at']['$date'], as_str=True) if event_dict['created_at'] != None else None
                found_events.append(event_dict)
        
        elif not event_sequence_event and profile_ids and not event_type: 
            raw_events = []
            query = {

                    '$and' : [
                        {'account_id' : { '$eq' : account_id}},
                        {'created_at' : { '$gt' : datetime_timeframe}},
                        {'profile_id' : { '$in' : profile_ids}}
                    ]
            }

            results_cursor = tbl_event_instances._get_collection().find(query).skip(cursor).limit(3000)
            total = tbl_event_instances._get_collection().find(query).count()
            raw_events.extend(json.loads(dumps(list(results_cursor), indent = 2)))
            found_events=[]
            for event in raw_events:
                event_dict = event
                event_dict['event_id'] = event_dict['_id']
                event_dict.pop('_id', None)
                event_dict['created_at'] = int_to_dt(event_dict['created_at']['$date'], as_str=True) if event_dict['created_at'] != None else None
                found_events.append(event_dict)

        elif not event_sequence_event and profile_ids and event_type: 
            raw_events = []
            query = {

                '$and' : [
                    {'account_id' : { '$eq' : account_id}},
                    {'created_at' : { '$gt' : datetime_timeframe}},
                    {'profile_id' : { '$in' : profile_ids}},
                    {'event_type' : { '$eq' : event_type}}
                ]
            }

            results_cursor = tbl_event_instances._get_collection().find(query).skip(cursor).limit(3000)
            total = tbl_event_instances._get_collection().find(query).count()
            raw_events.extend(json.loads(dumps(list(results_cursor), indent = 2)))
            found_events=[]
            for event in raw_events:
                event_dict = event
                event_dict['event_id'] = event_dict['_id']
                event_dict.pop('_id', None)
                event_dict['created_at'] = int_to_dt(event_dict['created_at']['$date'], as_str=True) if event_dict['created_at'] != None else None
                found_events.append(event_dict)

        else:
            events = tbl_event_instances.objects((Q(account_id__exact=account_id) & Q(created_at__gt=datetime_timeframe))).skip(cursor).limit(3000)
            total = tbl_event_instances.objects((Q(account_id__exact=account_id) & Q(created_at__gt=datetime_timeframe))).count()

            found_events=[]
            for event in events:
                event_dict = json.loads(event.to_json())
                event_dict['event_id'] = event_dict['_id']
                event_dict.pop('_id', None)
                event_dict['created_at'] = int_to_dt(event_dict['created_at']['$date'], as_str=True) if event_dict['created_at'] != None else None
                found_events.append(event_dict)

        return found_events, total

    async def delete_profile_events(self, account_id : str, profile_id : str) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_id : str
            octy profile identifier

        Returns
        ----------
        None
        """
        tbl_event_instances.objects(account_id__exact=account_id,profile_id__exact=profile_id).delete()


    async def get_profile_ids(self, account_id : str, profile_ids : list) -> Union[list,list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            list of profile ids to verfiy existence of

        Returns
        ----------
        valid profiles : list
        invalid profiles : list
        """
        valid_profiles = []
        invalid_profiles = []

        payload = {
            'account_id' : account_id,
            'profiles': profile_ids,
            'get_all' : False
        }
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids=true"
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                data=json.dumps(payload),
                timeout=60
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')

        if response.status_code == 400:
            return valid_profiles, invalid_profiles

        print(response.status_code)
        print(response.text)
        print(response)

        body = json.loads(response.text)
        for vprofile in body['profiles']:
            valid_profiles.append(vprofile)

        for ivprofile in body['not_found']:
            invalid_profiles.append(ivprofile)

        return valid_profiles, invalid_profiles

    async def get_live_segment_definitions(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account identifier

        Returns
        ----------
        found_segments : list
        """
        found_segments = []

        url = f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments?account_id={account_id}&status=active&segment_type=live"
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.get(
                url,
                timeout=60
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')

        if response.status_code == 400:
            return found_segments

        body = json.loads(response.text)
        for seg in body['segments']:
            found_segments.append(seg)
        
        return found_segments


eventsRepository = _EventsRepository()




'''
    async def get_events(self, account_id : str, profile_ids : list, timeframe : int, cursor : int) -> Union[list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            List of Octy profile identifiers
        timeframe : int
            number of days events must have occurred in
        cursor : int
            pagination cursor

        Returns
        ----------
        events : list
        total : int
        """
        found_events = []
        datetime_timeframe = dt.now() - td(days=timeframe+1) # NOTE: Add one additional day
        events = tbl_event_instances.objects((Q(profile_id__in=profile_ids) & \
            Q(account_id__exact=account_id) & Q(created_at__gt=datetime_timeframe))).skip(cursor).limit(2000)
        total = tbl_event_instances.objects((Q(profile_id__in=profile_ids) & \
            Q(account_id__exact=account_id) & Q(created_at__gt=datetime_timeframe))).count()
        
        for event in events:
            event_dict = json.loads(event.to_json())
            event_dict['event_id'] = event_dict['_id']
            event_dict.pop('_id', None)
            event_dict['created_at'] = int_to_dt(event_dict['created_at']['$date'], as_str=True) if event_dict['created_at'] != None else None
            found_events.append(event_dict)

        return found_events, total


'''

# payload = {
#     'account_id' : account_id,
#     'event_types': events
# }
# url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/events/meta"
# event_types = []

# t0 = time.time()
# try:
#     response = requests_retry_session().post(url,timeout=5, data=json.dumps(payload))
# except Exception as x:
#     print('Exception', x.__class__.__name__)
#     print(f'Error: {x}')
# else:
#     print('It eventually worked', response.status_code)
# finally:
#     t1 = time.time()
#     print('Took', t1 - t0, 'seconds')

# if response.status_code != 200:
#     return event_types

# body = json.loads(response.text)
# for event_type in body['events']:
#     event_types.append(
#         {
#             'event_type': event_type['event_type'],
#             'exists' : event_type['event_type'],
#             'event_properties': event_type['event_properties']
#         }
#     )
# return event_types, body['event_count']