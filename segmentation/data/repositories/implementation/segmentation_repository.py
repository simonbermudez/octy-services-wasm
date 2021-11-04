# module imports
from data.repositories.Isegmentation_repository import SegmentationInterface
from data.models.db_schemas import tbl_segments, EventSequence
from utils.utils import *
from api.routers.error_handlers import *


# python imports
from typing import *
import json
from datetime import datetime as dt
import time

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from pymongo.errors import BulkWriteError
from bson.json_util import dumps



class _SegmentationRepository(SegmentationInterface):
    """
        _SegmentationRepository
        Handles:
        - Get Segment definitions
        - Segment definitions creation
        - Delete Segment definitions

        ...

        Attributes
        ----------
        ...
    """
    def __init__(self): pass

    def get_segment_count(self, account_id : str) -> int:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        return tbl_segments.objects(account_id__exact=account_id).count()

    def get_segment_by_identifiers(self, identifiers : list, account_id : str) -> Union[list, int]:
        """
        Parameters
        ----------
        identifiers : list
            Segment identifier(s)
        account_id : str
            Octy account id

        Returns
        ----------
        segments : list
        total : int
        """
        query = [
            {"account_id" : { "$eq" : account_id}},
            {"status" : { "$eq" : "active"}}
        ]

        if identifiers != None:
            cursor = 0
            query.append(

                {
                    "$or" : [
                        {"_id" : { "$in" : identifiers}},
                        {"segment_name" : { "$in" : identifiers}}
                    ]
                    
                }
            
            )

        results_cursor = tbl_segments._get_collection().find({'$and' : query}).skip(cursor).limit(100)
        total = tbl_segments._get_collection().find({'$and' : query}).count()
        raw_res = json.loads(dumps(list(results_cursor), indent = 2))
        
        #format segments
        for segment in raw_res:
            segment['template_id'] = segment['_id']
            _format_segment(segment)

        return raw_res, total

    def get_segment_by_attr(self, account_id : str, segment : dict) -> dict:
        """
        Parameters
        ----------
        account_id : str
        segment : dict

        Returns
        ----------
        segment_dict : dict
        """
        segments = tbl_segments.objects((   Q(segment_name__exact=segment.segment_name) & Q(account_id__exact=account_id)))
        if segments:
            segment_dict = json.loads(segments[0].to_json())
            #segment_dict = segments[0]
            segment_dict['segment_id'] = segment_dict['_id']
            segment_dict = _format_segment(segment_dict)
            return segment_dict

        if not segment.profile_property_name and not segment.profile_property_value:


            segments = tbl_segments.objects(Q(account_id__exact=account_id) \
                    & Q(segment_type__exact=segment.segment_type) \
                    & Q(segment_sub_type__exact=segment.segment_sub_type) \
                    & Q(status__exact="active") )
        else:

            segments = tbl_segments.objects( Q(account_id__exact=account_id) \
                    & Q(segment_type__exact=segment.segment_type) \
                    & Q(segment_sub_type__exact=segment.segment_sub_type) \
                    & Q(profile_property_name__exact=segment.profile_property_name) \
                    & Q(profile_property_value__exact=segment.profile_property_value) \
                    & Q(status__exact="active") )
        
        es_json_list = []
        for es in segment.event_sequence:
            es_json_list.append(
                es.dict()
            )

        
        for found_segment in segments:
            segment_es_list=[]
            for event in found_segment.event_sequence:
                segment_es_list.append(
                    json.loads(event.to_json())
                )
            
            if segment_es_list == es_json_list:

                if found_segment.segment_type == segment.segment_type and \
                    found_segment.segment_sub_type == segment.segment_sub_type and \
                    found_segment.segment_timeframe == segment.segment_timeframe and \
                    found_segment.profile_property_name == segment.profile_property_name and \
                    found_segment.profile_property_value == segment.profile_property_value :
            
                    segment_dict = json.loads(found_segment.to_json())
                    segment_dict['segment_id'] = segment_dict['_id']
                    segment_dict = _format_segment(segment_dict)
                    return segment_dict
                else:
                    continue

        
        return None

    async def get_past_segments_by_profile_ids(self, account_id : str, profile_ids : list) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        segments : list
        """
        query = {
            '$and' : [
                {"account_id" : { "$eq" : account_id}},
                {"segment_type" : { "$eq" : "past"}},
                {"status" : { "$eq" : "active"}},
                {"profile_ids" : { "$in" : profile_ids}}
        ]}
        results_cursor = tbl_segments._get_collection().find(query)
        segments = json.loads(dumps(list(results_cursor), indent = 2))
        return segments

    def get_segments(self, account_id : str, segment_type : str, status : str, cursor : int, internal : bool = False) -> Union[list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_type : str
            live, past or all
        status : str
            the status to filter results by
        cursor : int
            Pagination cursor
        internal : bool
        Returns
        ----------
        found_segments, total : list, int
        """
        found_segments = []
        if status == 'all':
            if segment_type == 'all':
                segments = tbl_segments.objects(Q(account_id__exact=account_id)).skip(cursor).limit(100)
                total = tbl_segments.objects(Q(account_id__exact=account_id)).count()
            else:
                segments = tbl_segments.objects((Q(account_id__exact=account_id) & Q(segment_type__exact=segment_type))).skip(cursor).limit(100)
                total = tbl_segments.objects((Q(account_id__exact=account_id) & Q(segment_type__exact=segment_type))).count()

        else:
            if segment_type == 'all':
                segments = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id))).skip(cursor).limit(100)
                total = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id))).count()
            else:
                segments = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id) & Q(segment_type__exact=segment_type))).skip(cursor).limit(100)
                total = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id) & Q(segment_type__exact=segment_type))).count()

            # segments = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id))).skip(cursor).limit(100)
            # total = tbl_segments.objects((Q(status__exact=status) & Q(account_id__exact=account_id))).count()
        for segment in segments:
            segment_dict = json.loads(segment.to_json())
            segment_dict['segment_id'] = segment_dict['_id']
            segment_dict = _format_segment(segment_dict, internal=internal)
            found_segments.append(segment_dict)
        return found_segments, total

    def create_segment(self, segment : object) -> None:
        """
        Parameters
        ----------
        segment : CreateSegment
            CreateSegment request model instance

        Returns
        ----------
        None
        """
        event_sequence = []
        for es in segment['event_sequence']:
            event_sequence.append(
                EventSequence(
                    # event=es.event,
                    event_type=es.event_type,
                    exp_timeframe=es.exp_timeframe,
                    action_inaction=es.action_inaction,
                    event_properties=es.event_properties
                )
            )
        new_segment = tbl_segments(
            segment_id=segment['segment_id'],
            account_id=segment['account_id'],
            segment_name=segment['segment_name'],
            segment_type=segment['segment_type'],
            segment_sub_type=segment['segment_sub_type'],
            segment_timeframe=segment['segment_timeframe'],
            event_sequence=event_sequence,
            profile_property_name=segment['profile_property_name'],
            profile_property_value=segment['profile_property_value']
        )

        new_segment.save()

    async def update_segment_status(self, account_id : str, segment_ids : list, status : str) -> Union[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers
        status : str
            the status to filter results by
        Returns
        ----------
        pending_deleted_segments : List
        failed_to_delete : List
        """
        pending_deleted_segments = []
        failed_to_update = []

        #BULK UPDATE OPERATION
        bulk_operation = tbl_segments._get_collection().initialize_unordered_bulk_op()
        for s in segment_ids:
            bulk_operation.find({
                '$and' : [
                    {"_id" : { "$eq" : s['segment_id']}},
                    {"account_id" : { "$eq" : account_id}}
                ]
            }).update(
                {
                    "$set" : {"status" : status}
                }
            )

        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                mes = f"Unknown error occurred when updating segment with segment_id : {err['op']['u']['$set']['segment_id']}"
                failed_to_update.append({
                        'segment_id' : err['op']['u']['$set']['segment_id'],
                        'error_message' : mes
                    })


                pending_deleted_segments = list(filter(lambda i : i['segment_id'] != err['op']['u']['$set']['segment_id'], pending_deleted_segments))

        return pending_deleted_segments, failed_to_update

    async def update_past_segment_profile_ids(self, account_id : str, segment_id : str, profile_ids : list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_id : str
            segment identifier
        profile_ids : list
            List of octy profile identifiers

        Returns
        ----------
        None
        """
        tbl_segments.objects(Q(account_id__exact=account_id) \
            & Q(segment_id__exact=segment_id)).update(set__profile_ids=profile_ids)

    async def delete_segments(self, account_id : str, segment_ids : list) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        segment_ids : list
            list of segment identifiers

        Returns
        ----------
        None
        """

        bulk_operation = tbl_segments._get_collection().initialize_unordered_bulk_op()
        for segment in segment_ids:
            bulk_operation.find({
                '$and' : [
                    {  "_id" : { "$eq" : segment['segment_id'] }  },
                    {  "account_id" : { "$eq" : account_id }  }
                ]
            }).remove()

        bulk_operation.execute()

    def get_event_types_by_name(self, account_id : str, event_type_names : list) -> Union[list, list]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        event_type_names : list
            list of event_type names

        Returns
        ----------
        result : list, list
        """
        found_event_types = []
        not_found = []

        # if none supplied return empty lists
        if len(event_type_names) <1:
            return found_event_types, not_found

        payload = {
            'account_id' : account_id,
            'event_type_names': event_type_names
        }
        url = f"{Config['EVENT_SERVICE_CLUSTER_IP']}/v1/internal/events/types"
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
            return found_event_types, event_type_names

        body = json.loads(response.text)
        for et in body['event_types']:
            found_event_types.append(et)

        for ivet in body['not_found']:
            not_found.append(ivet)

        return found_event_types, not_found

segmentationRepository = _SegmentationRepository()

def _format_segment(segment : dict, internal : bool = False) -> dict:
    '''
        Format segment objects
    '''
    # Ensure Event Property keys is supplied with each event sequence object
    for event in segment['event_sequence']:
        try:
            event['event_properties']
        except KeyError:
            event['event_properties'] = None

    if not internal:    
        segment.pop('account_id', None)
        segment.pop('_id', None)
        segment.pop('id', None)

        if segment['segment_sub_type'] < 3:
            segment.pop('profile_property_name', None)
            segment.pop('profile_property_value', None)

        try:
            segment['created_at'] = int_to_dt(segment['created_at']['$date'], as_str=True) if segment['created_at'] != None else None
            # try:
            #     segment['updated_at'] = int_to_dt(segment['updated_at']['$date'], as_str=True) if segment['updated_at'] != None else None
            # except TypeError:
            #     segment['updated_at'] = segment['updated_at'].strftime('%a, %d %b %Y %H:%M:%S GMT')
        except KeyError:
            pass
        segment['profile_count'] = len(segment['profile_ids'])
        segment.pop('profile_ids', None)
        return segment
    segment.pop('_id', None)
    return segment
    