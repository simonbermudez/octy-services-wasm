# module imports
from data.repositories.implementation.segmentation_repository import segmentationRepository
from api.routers.request_models.segmentation import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
import json

# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from fastapi import Request


class SegmentValidatation():
    """
        SegmentValidatation
        Handles:
        - Segment definitions validation rules
        ...

        Attributes
        ----------
        account : Octy account
        segment : CreateSegment
    """

    def __init__(self, account : Account, segment : CreateSegment) :
        self.account = account
        self.segment = segment
        self.event_sequence_limit = 10
        self.segment_types = ['live', 'past']
        self.past_segment_sub_types = [1,2,3,4]
        self.past_segment_intervals = 2
        self.live_segment_sub_types = [1,2]
        self.num_of_events = len(self.segment.event_sequence)
        self.last_idx = self.num_of_events - 1
        self.provided_custom_event_types = []
        self.system_event_types = []
        for et in Config['SYSTEM_EVENT_TYPES_MAP']:
            self.system_event_types.append(et['event_type'])
    
    # Shared validations ---
    async def _v_num_of_events(self) -> None:
        if len(self.segment.event_sequence) > self.event_sequence_limit:
            raise OctyException(400,'Invalid event sequence provided.', 
                [{'message' : f'The event sequence provided exceeds the maximun limit of {str(self.event_sequence_limit)} events', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
    
    async def _v_segment_type(self) -> None:
        if self.segment.segment_type not in self.segment_types:
            raise OctyException(400,'Invalid segment subtype provided.', 
                [{'message' : f'segment_type must be type of \'live\' or \'past\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
    
    async def _v_segment_duplicates(self) -> None:
        exisitng_segment = segmentationRepository.get_segment_by_attr(self.account.account_id, self.segment)
        if exisitng_segment:
            # determine why segment is duplicated
            # Check for duplicte name
            if exisitng_segment['segment_name'] == self.segment.segment_name:
                raise OctyException(400,'Duplicate segment name provided.', 
                    [{'message' : f'Segment with provided name: {self.segment.segment_name} already exists. If you have recently deleted a segment with this name, you must wait up to 72 hours before you are able to create another segment with this name.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            # duplicate type, sub type, segment_timeframe and event list
            else:
                raise OctyException(400,'Duplicate segment type, sub type and event sequence provided.', 
                    [{'message' : f'Segment with provided type: {self.segment.segment_type}, sub type: {self.segment.segment_sub_type} with an identical event sequence, segment_timeframe and profile properties already exists.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_event_sequence_event_types(self) -> None:
        for _, event in enumerate(self.segment.event_sequence):
            if event.event_type not in self.system_event_types:
                self.provided_custom_event_types.append(event.event_type)
        
        found_event_types, _ = segmentationRepository.get_event_types_by_name(self.account.account_id, \
            self.provided_custom_event_types)
        
        for _, event in enumerate(self.segment.event_sequence):
            if event.event_type in self.system_event_types:
                # event.event_type = event.event
                system_event=next((key for key in Config['SYSTEM_EVENT_TYPES_MAP'] if key['event_type'] == event.event_type), None)
                if event.event_properties:
                    for k, v in event.event_properties.items():
                        if k not in system_event['event_properties']:
                            raise OctyException(400, f"Invalid event provided within the event sequence of this request. The system event type '{event.event_type}' does not have a key named '{k}' in it's event_properties attribute.", 
                                [{'message' : 'Invalid event provided.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
                        else:
                            # Check provided data type of event property value
                            if type(v) != type(system_event['event_properties'][k]):
                                raise OctyException(400, f"Invalid event provided within the event sequence of this request. The system event type '{event.event_type}' event_properties key '{k}' value must be of type : {type(system_event['event_properties'][k])}", 
                                    [{'message' : 'Invalid event provided.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
                
                continue
            custom_event=next((key for key in found_event_types if key['event_type'] == event.event_type), None)
            if custom_event:
                # event.event_type = custom_event['event_type']
                if event.event_properties:
                    for k,_ in event.event_properties.items():
                        if k not in custom_event['event_properties']:
                            raise OctyException(400, f"Invalid event provided within the event sequence of this request. The custom event type '{event.event_type}' does not have a key named '{k}' in it's event_properties attribute.", 
                                [{'message' : 'Invalid event provided.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            else:
                raise OctyException(400, f'Invalid event provided within the event sequence of this request. Event \'{event.event_type}\' does not exist, with provided event_properties.', 
                    [{'message' : 'Invalid event provided.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
 
    async def _v_event_sequence_duplicates(self) -> None:
        #Ensure no duplicates are provided in event sequence
        #can be duplicates keys, but not duplicates keys with the sames values provided for key : value pair event_properties
        duplicates=[]# track duplicate keys in event sequence
        events_list=[]#track if event has occurred previously in event sequence
        for _, event in enumerate(self.segment.event_sequence):
            if event.event_type in events_list:
                #if event already exists in events_list, we have a duplicate!
                exists=next((e for e in duplicates if e == event.event_type), None)
                if exists==None:
                    #add to duplicates list 
                    duplicates.append(event.event_type)
                else:
                    continue
            else:
                events_list.append(event.event_type)

        if len(duplicates)>0:
            #We have duplicate keys, which is ok, as long as we don't have duplicate keys with duplicate event property values.
            event_prop_map_dict={} #map :: {"event prop key" : [all, values, associated, with, this, key]
            event_prop_list=[]
            event_prop_none_list=[] # list of events that have occurred with None set as event_property parameter
            key_ = ""
            #iterate over event_sequence
            for event in self.segment.event_sequence:
                #populate event_prop_map if event exists in duplicates
                if event.event_type in duplicates:
                    #if no event properties provided, determine if this event with null event properties exists 
                    if event.event_properties == None:

                        exists=next((e for e in event_prop_none_list if e == event.event_type), None)
                        if exists == None:
                            event_prop_none_list.append(event.event_type)
                        else:
                            #if event exists with None set as as event_property parameter in event_prop_none_list, return error
                            raise OctyException(400, 'Invalid event sequence provided.', 
                                [{'message' : 'Duplicate events with matching event properties found in event sequence.', 
                                'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
                        continue

                    #If event properties parameter contains key value objects:
                    #iterate over event property keys for this event
                    for prop in event.event_properties:
                        key_=prop
                        exists=next((e for e in event_prop_list if e == prop), None)
                        if exists == None:
                            #if key (prop) does not exists in event_prop_map, append new kay value pair
                            event_prop_map_dict[prop] = [event.event_properties[prop]]
                            event_prop_list.append(prop)
                        else:
                            #if key exists in event_prop_map, simply append value to list
                            event_prop_map_dict[prop].append(event.event_properties[prop])

            if len(event_prop_map_dict[key_]) != len(set(event_prop_map_dict[key_])):
                raise OctyException(400, 'Invalid event sequence provided.', 
                    [{'message' : 'Duplicate events with matching event properties found in event sequence.', 
                    'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])


    # Past segment validations ---
    async def _v_past_subtype(self) -> None:
        if self.segment.segment_sub_type not in self.past_segment_sub_types:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'Past segments must have a sub type of either 1,2,3 or 4', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_past_timeframe(self) -> None:
        if self.segment.segment_timeframe < self.past_segment_intervals or self.segment.segment_timeframe > 365:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : f'Past segments must have a timeframe of more than {self.past_segment_intervals} days and less than 365.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_past_profile_properties(self) -> None:
        if self.segment.profile_property_name != None and self.segment.profile_property_value == None:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'profile_property_value must be provided with profile_property_name', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
        
        if self.segment.profile_property_name == None and  self.segment.profile_property_value != None:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'profile_property_name must be provided with profile_property_value', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        if self.segment.segment_sub_type == 3 or self.segment.segment_sub_type == 4:
            if self.segment.profile_property_name == None or \
                self.segment.profile_property_value == None:
                raise OctyException(400,'Invalid segment provided.', 
                    [{'message' : f'Past segments with a sub type 3 or 4 must have both profile_property_name and profile_property_value parameters provided', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
                
        if self.segment.segment_sub_type == 1 or self.segment.segment_sub_type == 2:
            if self.segment.profile_property_name != None or \
                self.segment.profile_property_value != None:
                raise OctyException(400,'Invalid segment provided.', 
                    [{'message' : f'Past segments with a sub type 1 or 2 must not have either profile_property_name or profile_property_value parameters provided', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
                
    async def _v_past_event_sequence_event_timeframes(self) -> None:
        # Assess event sequence event timeframes
        for _, event in enumerate(self.segment.event_sequence):
            # exp_timeframe must not be more than 0
            if event.exp_timeframe != 0:
                raise OctyException(400,'Invalid event provided.', 
                    [{'message' : 'The \'exp_timeframe\' parameter within each \'event_sequence\'>>\'event\' object MUST be set to 0 if the \'segment_type\' parameter is set to \'past\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
        
    async def _v_past_event_sequence(self) -> None:
        # Must be at least one action. First event should always be an action
        if self.segment.event_sequence[0].action_inaction != 'action':
            raise OctyException(400,'Invalid event sequence provided.', 
                [{'message' : 'The first event \'action_inaction\' parameter in a segments event sequence must be of type \'action\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        # Must be an inaction. This inaction must be at the last index if subtype 2 or 4
        if self.segment.segment_sub_type == 2 or self.segment.segment_sub_type == 4:
            if self.segment.event_sequence[self.last_idx].action_inaction != 'inaction':
                raise OctyException(400,'Invalid event sequence provided.', 
                    [{'message' : 'The last event \'action_inaction\' parameter in segments with sub type 2 or 4 must be of type \'inaction\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

            # Must not be more than one inaction event in event sequence
            filtered = await self._filter_action_inaction('inaction', self.segment.event_sequence)
            if len(filtered) > 1:
                raise OctyException(400,'Invalid event sequence provided.', 
                    [{'message' : 'Segments can contain no more than one single \'inaction\' event in their event sequence.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        else:
            # must NOT be any inactions if subtype 1 or 3
            inaction_event = next((event for event in self.segment.event_sequence if event.action_inaction == 'inaction'), None)
            if inaction_event:
                raise OctyException(400,'Invalid event sequence provided.', 
                    [{'message' : 'Sub type 1 & 3 segments can not contain inaction events in their event sequence.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        await self._v_event_sequence_event_types()
        await self._v_event_sequence_duplicates()
        await self._v_past_event_sequence_event_timeframes()
        
    
    # Live segment validations ---
    async def _v_live_subtype(self) -> None:
        if self.segment.segment_sub_type not in self.live_segment_sub_types:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'Live segments must have a sub type of either 1 or 2', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_live_timeframe(self) -> None:
        if self.segment.segment_timeframe != 0:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'When creating a \'live-segment\' definition, the \'segment_timeframe\' parameter must have a value of 0', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            
    async def _v_live_profile_properties(self) -> None:
        if self.segment.profile_property_name != None or self.segment.profile_property_value != None:
            raise OctyException(400,'Invalid segment provided.', 
                [{'message' : 'profile_property_name or profile_property_value protperties must not be provided when creating a live segment', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_live_event_sequence_event_timeframes(self) -> None:
        # Assess event sequence event timeframes
        for idx, event in enumerate(self.segment.event_sequence):
            if idx != self.num_of_events-1:
                if event.exp_timeframe < 2:
                    raise OctyException(400,'Invalid event provided.', 
                        [{'message' : 'The \'exp_timeframe\' parameter within the first \'event_sequence\'>>\'event\' object MUST be set to \'2\' or more (minutes).', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

            last_event = self.segment.event_sequence[self.last_idx]
            if last_event.exp_timeframe > 0:
                raise OctyException(400,'Invalid event provided.', 
                    [{'message' : 'The \'exp_timeframe\' parameter within the last \'event_sequence\'>>\'event\' object MUST be set to 0.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

    async def _v_live_event_sequence(self) -> None:
        # Must be at least one action. First event should always be an action
        if self.segment.event_sequence[0].action_inaction != 'action':
            raise OctyException(400,'Invalid event sequence provided.', 
                [{'message' : 'The first event \'action_inaction\' parameter in a segments event sequence must be of type \'action\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        # Must not be more than one action event in event sequence
        filtered = await self._filter_action_inaction('action', self.segment.event_sequence)
        if len(filtered) > 1:
            raise OctyException(400,'Invalid event sequence provided.', 
                [{'message' : 'Live segments can contain no more than one single \'action\' event in their event sequence.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            
        if self.segment.segment_sub_type == 2:
            # must be an inaction. This inaction must be at the last index if subtype 2
            if self.segment.event_sequence[self.last_idx].action_inaction != 'inaction':
                raise OctyException(400,'Invalid event sequence provided.', 
                    [{'message' : 'The last event \'action_inaction\' parameter in segments with sub type 2 , must be of type \'inaction\'', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
        else:
            # must NOT be any inactions if subtype 1
            inaction_event = next((event for event in self.segment.event_sequence if event.action_inaction == 'inaction'), None)
            if inaction_event:
                raise OctyException(400,'Invalid event sequence provided.', 
                    [{'message' : 'Sub type 1 segments can not contain inaction events in their event sequence.', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        await self._v_event_sequence_event_types()
        await self._v_event_sequence_duplicates()
        await self._v_live_event_sequence_event_timeframes()
    

    # helper methods ---
    async def _filter_action_inaction(self, action_inaction : str, event_sequence : dict) -> list:
        return list(filter(lambda x : x.action_inaction == action_inaction, event_sequence))

    # Core method ---
    async def validate(self) -> object:
        # Shared validations
        await self._v_segment_type()
        await self._v_num_of_events()

        # switch through segment type
        if self.segment.segment_type == 'past':
            # past segment validations
            await self._v_past_subtype()
            await self._v_past_timeframe()
            await self._v_past_profile_properties()
            await self._v_past_event_sequence()

        elif self.segment.segment_type == 'live':
            # live segment validations
            await self._v_live_subtype()
            await self._v_live_timeframe()
            await self._v_live_profile_properties()
            await self._v_live_event_sequence()

        # Shared validation
        await self._v_segment_duplicates()

        return self.segment

class SegmentationService():
    """
        SegmentationService
        Handles:
        - Get Segment definitions
        - Segment definitions creation
        - Delete Segment definitions
        ...

        Attributes
        ----------
        account : Octy account
        account_id : str
    """
    def __init__(self, account : Account, account_id : str = None):
        self.account = account
        self.account_id = account_id if account_id != None else account.account_id

    def get_segments(self,
                  identifiers : list = None, 
                  cursor : int = None, 
                  status='active',
                  segment_type='all',
                  internal=False) -> Union[list, int]:
        
        """
        Parameters
        ----------
        identifiers : list
            list of segment identifiers. segment_id(s) or friendly name(s)
        cursor : int
            Pagination cursor
        status : str
            desired status of returned segments
        internal : bool

        Returns
        ----------
        segments : list
        total : int
        """
        if identifiers != None and cursor == 0:
            segments, total = segmentationRepository.get_segment_by_identifiers(identifiers=identifiers,account_id=self.account_id)
            if total<1:
                raise OctyException(400, 'Invalid segment identifier(s) provided', 
                [{'message' : 'No segments were found with the provided identifier(s)', 
                'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            
            return segments, total
            

        elif identifiers == None and cursor != None:
            
            segments,total = segmentationRepository.get_segments(account_id=self.account_id, 
                                                segment_type=segment_type,
                                                status=status,
                                                cursor=cursor, 
                                                internal=internal)
            if len(segments)<1:
                raise OctyException(400, 'No segments found', 
                    [{'message' : 'No segments found with the provided segment identifier or pagination cursor exhausted', 
                    'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            return segments, total

    async def _create_segment_ref(self, segment) -> str:
            #Record segment definition
            new_segment = {
                'segment_id' : generate_uid('segment'),
                'account_id' : self.account.account_id,
                'segment_name' : segment.segment_name,
                'segment_type' : segment.segment_type,
                'segment_sub_type' : segment.segment_sub_type,
                'segment_timeframe' : segment.segment_timeframe,
                'event_sequence' : segment.event_sequence,
                'profile_property_name' : segment.profile_property_name,
                'profile_property_value' : segment.profile_property_value,
                'segment_status' : 'created' if segment.segment_type == 'live' else 'processing'
            }
            segmentationRepository.create_segment(new_segment)
            return new_segment

    async def create_segment(self, segment : CreateSegment) -> dict:
        """
        Parameters
        ----------
        segment : CreateSegment
            CreateSegment request model instance

        Returns
        ----------
        Created segment : dict
        """

        # assess allowed limits
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              segmentationRepository.get_segment_count(self.account.account_id), 1)
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'message' : f'This request could not be completed as this request exceeds the allowed number of segments : {counts["limit"]}. This account can create another {counts["remainder"]} segment(s).', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])
        
        segment = await SegmentValidatation(account=self.account, segment=segment).validate()

        # Establish segment primary type live | past
        #LIVE SEGMENTATION
        if segment.segment_type == 'live':
            seg = await self._create_segment_ref(segment)
            #Return response
            return seg, 'Segment created'

        #PAST SEGMENTATION
        elif segment.segment_type == 'past':
            seg = await self._create_segment_ref(segment)
            
            await amqpPublisher.send_message(routing_key='octy.job.cmd.create',
                payload={
                    'account_id' : self.account.account_id,
                    'alt_dentifier' :seg['segment_id'],
                    'job_meta' : {
                        'job_type' : 'seg',
                        'amqp_routing_key': 'past.segmentation.cmd.run',
                        'required_permissions' : ['seg'],
                        'required_configurations' :
                            { 
                                'account_attributes' : [
                                    'account_configurations.webhook_url'
                                ],
                                'algorithm_configuration_idxs' : [
                                ]
                            },
                        'desired_runs' : 0,
                        'time_interval' : Config['PAST_SEGMENTATION_JOB_INTERVAL'],
                        'fail_threshold' : 0
                    },
                    'job_data' : {
                        'segmentation_type' : 'past',
                        'segment_id' : seg['segment_id']
                    }
            })
            #Return response from segmentation engine initalisation, not completion
            return seg, 'Segmentation process initiated'
   
    async def _filter_segments(self, segments, segment_id):
        return list(filter(lambda x : x['segment_id'] == segment_id, segments[0]))

    async def update_past_segment_profiles(self, profiles : list) -> None:
        """
        Parameters
        ----------
        profiles : list
            List of parent and their respective child profiles

        Returns
        ----------
        None
        """

        def _child_to_parent(profile_id) -> str: 
            '''
            if profile_id is a child,
            return childs corresponding parent profile id
            or None if child not found
            '''
            profile = next((p for p in profiles if profile_id in p.child_profiles), None)
            if profile != None:
                return profile.parent_profile
            return None

        # Get all child profiles
        all_child_profile_ids = list()
        [all_child_profile_ids.extend(cp for cp in p.child_profiles) for p in profiles]

        segments = await segmentationRepository\
            .get_past_segments_by_profile_ids(account_id=self.account_id, profile_ids=all_child_profile_ids)
        for segment in segments:
            current_segment_profile_ids = segment['profile_ids']
            for i, profile in enumerate(current_segment_profile_ids):
                parent = _child_to_parent(profile)
                if parent:
                    current_segment_profile_ids[i] = parent
            updated_segment_profile_ids = list(dict.fromkeys(current_segment_profile_ids))
            await segmentationRepository\
                .update_past_segment_profile_ids(account_id=self.account_id, 
                                                segment_id=segment['_id'], 
                                                profile_ids=updated_segment_profile_ids)

    async def delete_segments(self, segment_ids : list) -> Union[list, list]:
        """
        Parameters
        ----------
        segment_ids : list
            List of segment ids that should be deleted

        Returns
        ----------
        deleted_segments : list
        failed_to_delete : list
        """
        deleted_segments = []
        failed_to_delete = []
        de_duped_segment_ids=[]

        # deduplicate segment_ids
        for segment_id in segment_ids:
            if segment_id not in de_duped_segment_ids:
                de_duped_segment_ids.append(segment_id)

        segments = segmentationRepository.get_segments(account_id=self.account_id, 
                                                segment_type='all',
                                                status='active', 
                                                cursor=0)
        if len(segments)<1:
            raise OctyException(400,'No segments found', 
                [{'message' : 'No active segments found associated with this account', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])
            
        for segment_id in de_duped_segment_ids:

            #Determine if segments exists
            does_exist = await self._filter_segments(segments, segment_id)
            if not does_exist:
                failed_to_delete.append(
                    {
                        'segment_id' : segment_id,
                        'error_message' : 'No active segment definitions found with this segment_id'
                    }
                )
            else:
                deleted_segments.append(
                    {
                        'segment_id' : segment_id
                    }
                )

        if len(deleted_segments) < 1:
            raise OctyException(400,'Invalid segment id provided', 
                [{'message' : 'No segments found with provided segment_ids', 'extended_help': Config['SEGMENTATION_EXTENDED_HELP']}])

        #Delete segment definition
        await segmentationRepository.delete_segments(self.account_id, deleted_segments)
        #Delete all segment tags associated with current segment
        await amqpPublisher.send_message(routing_key='segment.tags.cmd.update.delete',
            payload={
                "account_id" : self.account.account_id,
                "action" : "delete",
                "segment_ids" : deleted_segments
            })



        #NOTE: AMQP call to delete past segmentation job from octy-job service task list
        _ids = []
        for seg in deleted_segments:
            _ids.append(seg['segment_id'])
        await amqpPublisher.send_message(routing_key='octy.job.cmd.delete',
            payload={
                "account_id" : self.account.account_id,
                "octy_job_ids" : None,
                "alt_identifiers" : _ids
            })


        return deleted_segments, failed_to_delete
