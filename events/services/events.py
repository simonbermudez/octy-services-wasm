# module imports
from data.repositories.implementation.events_repository import eventsRepository
from data.repositories.implementation.event_types_repository import eventTypesRepository
from api.routers.request_models.events import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config
from .billing import BillingUnits

# python imports
from typing import *
from datetime import datetime as dt
import json

# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher

class EventsService():
    """
        EventsService
        Handles:
        - Create event
        - Batch create events
        - Get events for all all specified profiles
        - Get event meta data
        ...

        Attributes
        ----------
        account : Octy account
        account_id : str
    """

    def __init__(self, account : Account = None, account_id : str = None): 
        self.account = account
        self.account_id = account_id if account_id != None else account.account_id
        self.b = None if self.account is None else BillingUnits(account_id=self.account.account_id, account_type=self.account.account_configurations['a_t'], account_currency=self.account.account_configurations['a_c'], process_name='events_data')
    
    async def create_event(self, event : CreateEvent) -> dict:
        """
        Parameters
        ----------
        event : CreateEvent
            CreateEvent request model instance

        Returns
        ----------
        event : dict
        """
        # validate event. if invalid raise Octy error 400
        # assess allowed limits

        # if event type is includes ip address, cart token and customer info , use it to configure    system event type, ensure event properties are valid

        latest_events, event_count = await eventsRepository.get_events_meta(account_id=self.account.account_id, event_type_list=[event.event_type])
        count_res, counts = assess_resource_limit(self.account.account_configurations['li'],event_count,1,resource_key=3)
        if not count_res:
            raise OctyException(400,'Resource limit exceeded', 
                [{'error_message' : f'This request could not be completed as the number of events sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} events.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])
        

        # get latest events matching this event types
        try:
            le = latest_events[0]
        except IndexError:
            le = None

        # verify provided profile id exists
        valid_profiles, invalid_profiles = await eventsRepository.get_profile_ids(account_id=self.account.account_id, profile_ids=[event.profile_id])

        if len(invalid_profiles)> 0 or len(valid_profiles)<1:
            raise OctyException(400,'Invalid event data provided', 
                [{'error_message' : 'Unknown profile_id supplied with this event instance', 'extended_help': Config['EVENTS_EXTENDED_HELP']}])
        

        # verify event
        res, err_msg, event_type_id = await self._verify_event(event.event_type, event.event_properties, le, profile_id=event.profile_id, ivps=invalid_profiles)
        if res==False:
            if 'server error' in err_msg[1]:
                raise Exception(500)
            raise OctyException(400, err_msg[1], [{'error_message' : err_msg[0], 'extended_help': Config['EVENTS_EXTENDED_HELP']}])

        event_id = generate_uid('event')
        created_event = {

            'event_id' : event_id,
            'profile_id' : event.profile_id,
            'event_type_id' : event_type_id,
            'event_type' : event.event_type,
            'event_properties' : event.event_properties
        }

        await eventsRepository.create_event(self.account.account_id, created_event)

        await self.b.track_data_units(created_event)
        await self.b.complete_data_units('MB')

        # NOTE: Only create octy job for this event if event_type is in an active live segment event sequence.
        segments = await eventsRepository.get_live_segment_definitions(self.account.account_id)
        segment_event_types = []
        for segment in segments:
            for ev in segment['event_sequence']:
                if ev['event_type'] not in segment_event_types:
                    segment_event_types.append(ev['event_type'])

        if event.event_type in segment_event_types:
            # make AMQP call to init live segmentation worker
            await amqpPublisher.send_message(routing_key='octy.job.cmd.create',
                payload={
                    'account_id' : self.account.account_id,
                    'job_meta' : {
                        'job_type' : 'seg',
                        'amqp_routing_key': 'live.segmentation.cmd.run',
                        'required_permissions' : ['seg'],
                        'required_configurations' :
                            { 
                                'account_attributes' : [
                                    'account_configurations.webhook_url'
                                ],
                                'algorithm_configuration_idxs' : [
                                ]
                            },
                        'desired_runs' : 1,
                        'time_interval' : 0,
                        'fail_threshold' : 10
                    },
                    'job_data' : {
                        'segment_data' : {
                            'segmentation_type': 'live'
                        },
                        'validation_job' : False,
                        'event_data' : {
                            'event_id' : event_id,
                            'event_type_id' : event_type_id,
                            'event_type' : event.event_type,
                            'event_properties' : event.event_properties,
                            'profile' : valid_profiles[0]
                        }
                    }
            })

        return created_event
    
    async def batch_create_events(self, events : BatchCreateEvents) -> Union[list, list]:
        """
        Parameters
        ----------
        events : BatchCreateEvents
            BatchCreateEvents request model instance

        Returns
        ----------
        valid events : list (not absolutley valid -- any invalid profiles will create events for!)
        invalid events : list
        """
        valid_events = []
        ret_valid_events = []
        invalid_events = []
        event_types = []
        profile_ids = []

        for event in events.events:
            if event.event_type not in event_types:
                event_types.append(event.event_type)
            profile_ids.append(event.profile_id)

        # validate events
        # assess allowed limits
        latest_events, event_count = await eventsRepository.get_events_meta(account_id=self.account.account_id, event_type_list=event_types)
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              event_count,
                              len(events.events),resource_key=3)
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'error_message' : f'This request could not be completed as the number of events sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} events.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        # verify provided profile ids exists
        valid_profiles, invalid_profiles = await eventsRepository.get_profile_ids(account_id=self.account.account_id, profile_ids=profile_ids)
        if len(valid_profiles)<1:
                raise OctyException(400,'Invalid event data provided', 
                    [{'error_message' : 'No valid profile_id(s) were supplied with event instance(s)', 'extended_help': Config['EVENTS_EXTENDED_HELP']}])

        for event in events.events:
            
            #if created_at is provided, ensure format is correct and convert to datetime object
            created_at=None
            try:
                if event.created_at == None or event.created_at == '':
                    created_at=dt.now()
                else:
                    try:
                        created_at=dt.strptime(event.created_at, '%Y-%m-%d %H:%M:%S')
                    except ValueError:
                        invalid_events.append({
                            'event_type' : event.event_type,
                            'event_properties' : event.event_properties,
                            'profile_id' : event.profile_id,
                            'error_message' : 'Incorrect date format supplied, should be YYYY-MM-DD HH:MM:SS'
                        })
                        continue
            except KeyError:
                created_at=dt.now()
            
            le = None
            if latest_events:
                le=next((key for key in latest_events if key['event_type'] == event.event_type), None)


            #verify event
            res, err_msg, event_type_id = await self._verify_event(event.event_type, event.event_properties, le, profile_id=event.profile_id, ivps=invalid_profiles)
            if res==False:
                invalid_events.append({
                        'event_type' : event.event_type,
                        'event_properties' : event.event_properties,
                        'profile_id' : event.profile_id,
                        'error_message' : err_msg[0]
                })
                continue
            
            event_id = generate_uid('event')
            valid_events.append(
                {
                    'event_id': event_id,
                    'profile_id':event.profile_id,
                    'event_type_id': event_type_id,
                    'event_type':event.event_type,
                    'event_properties':event.event_properties,
                    'created_at': created_at
                }
            )
            ret_valid_events.append(
                {
                    'event_id': event_id,
                    'profile_id':event.profile_id,
                    'event_type_id': event_type_id,
                    'event_type':event.event_type,
                    'event_properties':event.event_properties,
                    'created_at': created_at.strftime('%a, %d %b %Y %H:%M:%S GMT')
                }
            )

        if len(valid_events)< 1:
            # raise OctyException(400,'Invalid events data provided. No events were created.', 
            # [{'message' : f'No events were created, due to invalid event data being provided. Please ensure required event properties with values of the correct data type are provided with each event.', 'extended_help': Config['EVENTS_EXTENDED_HELP']}])
            raise OctyException(400,'Invalid events data provided. No events were created.', invalid_events)
        
        await self.b.track_data_units(valid_events)
        await self.b.complete_data_units('MB')
        
        await eventsRepository.batch_create_events(self.account.account_id, valid_events)

        return ret_valid_events, invalid_events

    async def _verify_event(self, event_type : str, event_properties : dict, latest_event : dict, profile_id : str, ivps : list) -> Union[bool, str, str]:
        """
        Parameters
        ----------
        event_type : str
            Octy account id
        event_properties : list[str]
            provided event properties
        latest_event : dict
            latest event instances 
        profile_id : str
            events profile_id
        vps : list
            list of valid profile ids 
        ivps : list
            list of invalid profile ids 

        Returns
        ----------
        result, err_msg, event_type_id
        """
        err_msg=[]
        event_type_id=None

        # Verify profile_id is valid
        if profile_id in ivps: 
            err_msg.extend(['Unknown profile_id supplied with this event instance', 'Invalid event data provided'])
            return False, err_msg, None
    
        # check if event type is system event type 
        if event_type in Config['SYSTEM_EVENT_TYPES']:
            # Depending on system event type we need to execute a pre determined action.
            if event_type == 'charged':

                # make AMQP call to Update customer profile 'has_charged' == True
                await amqpPublisher.send_message(routing_key='profiles.cmd.update',
                payload={
                    "account_id" : self.account.account_id,
                    "profiles" : [
                        {
                            'profile_id' : profile_id,
                            'has_charged' : True
                        }
                    ]
                })

                event_type_id = event_type

                #determine if charge event has a 'payment method' and 'item_id' within 'event_properties'
                payment_method=None
                item_id=None
                for k,v in event_properties.items():

                    #check values provided are of type string
                    if not isinstance(v, str):
                        err_msg.extend(['Event type \'charged\'. The values provided for the \'payment_method\' and \'item_id\' event properties must be of type string. This charge has been logged against the customers profile but will not be used in any training jobs.','Invalid event data provided'])
                        return False, err_msg, None

                    try:
                        if k == 'payment_method':
                            if v == None:
                                continue
                            if len(v) > 1:
                                payment_method = event_properties[k]
                        elif k == 'item_id':
                            if v == None:
                                continue
                            if len(v) > 1:
                                item_id = event_properties[k]
                    except KeyError:
                        err_msg.extend(['Events of type \'charged\' must be provided with \'payment_method\' and \'item_id\' parameters within the event_properties. This charge has been logged against the customers profile but will not be used in any training jobs.','Invalid event data provided'])
                        return False, err_msg, None
                if payment_method==None or item_id==None:
                    err_msg.extend(['Events of type \'charged\' must be provided with \'payment_method\' and \'item_id\' parameters within the event_properties. This charge has been logged against the customers profile but will not be used in any training jobs.', 'Invalid event data provided'])
                    return False, err_msg, None

            elif event_type == 'churned':
                
                # make AMQP call to Update customer profile 'status' == 'churned', if profile is active.
                await amqpPublisher.send_message(routing_key='profiles.cmd.update',
                payload={
                    "account_id" : self.account.account_id,
                    "profiles" : [
                        {
                            'profile_id' : profile_id,
                            'status' : 'churned'
                        }
                    ]
                })

                event_type_id = event_type

            elif event_type == 'complaint':
                #determine if complaint event has a 'channel' within 'event_properties'
                channel=None
                for k,v in event_properties.items():

                    #check values provided are of type string
                    if not isinstance(v, str):
                        err_msg.extend(['Event type \'complaint\'. The values provided for the \'channel\' event property must be of type string.','Invalid event data provided'])
                        return False, err_msg, None

                    try:
                        if k == 'channel':
                            if v == None:
                                continue
                            if len(v) > 1:
                                channel = event_properties[k]
                    except KeyError:
                        err_msg.extend(['Events of type \'complaint\' must be provided with a \'channel\' parameter within the event_properties.', 'Invalid event data provided'])
                        return False, err_msg, None
                if channel==None:
                    err_msg.extend(['Events of type \'complaint\' must be provided with a \'channel\' parameter within the event_properties.', 'Invalid event data provided'])
                    return False, err_msg, None
                event_type_id = event_type

        # if not system event type ensure event type and event properties exist
        else:
            
            custom_event_type_exist = eventTypesRepository.get_event_type_by_name(account_id=self.account.account_id, event_type=event_type)

            # ensure event type provided is a valid custom event type
            if not custom_event_type_exist:
                err_msg.extend(['Unknown event type supplied with this request.', 'Invalid event_type.'])
                return False, err_msg, None

            event_type_id=custom_event_type_exist['event_type_id']

            #ensure all event property keys are provided.  Please ensure all event property keys have been provided.
            event_instance_exists = True
            if not latest_event:
                event_instance_exists = False
            
            # a map used to assess provided ep's (keys and value types)
            required_event_properties_map = list()
            property_type_match=True

            for ep in custom_event_type_exist['event_properties']:
                #only need to assess types if a previous event instance of this type exists
                if event_instance_exists:
                    #assess required data type
                    try:

                        if not isinstance(event_properties[ep], \
                        type(latest_event['event_properties'][ep])):
                            property_type_match=False
                    except KeyError:
                        #in the event there is not data type reference, this event will set it.
                        pass

                #populate map of required event properties
                required_event_properties_map.append({
                        'property' : ep,
                        'provided' : True if ep in event_properties else False,
                        'property_type_match' : property_type_match
                })

            
            #evaluate map
            for rep in required_event_properties_map:
                if not rep['provided']:
                    #return err
                    err_msg.extend([f'Please provide all required event properties key value pairs for this event type. Missing event property key : \'{rep["property"]}\'','Invalid event data provided'])
                    return False, err_msg, None
                if not rep['property_type_match']:
                    #return err
                    err_msg.extend(['Invalid data types specified for one or more of the provided event properties.','Invalid event data provided'])
                    return False, err_msg, None


        
        #get algorithm configurations for this account
        #iterate over OCTY_ALGO_TYPES to validate item_identifiers
        for algo_conf in self.account.algorithm_configurations:
            try:
                config = algo_conf['config_json']
                if len(config) == 0:
                    continue
            except KeyError:
                continue
            
            #determine if event_type is in algo config '{algo}_event_type'
            et = 'event_type'
            iid = algo_conf['algorithm_name']+'_item_identifier'
            if event_type == config[et]:
                #ensure event_properties contains 'item_id'(set rec_item_identifier) key and value is not null or empty string
                try: 
                    item_identifier=event_properties[config[iid]]
                    if item_identifier == None or item_identifier == "":
                        err_msg.extend(['event_properties -- {a} can not contain a null value as this event is a primary event type.'.format(a=config[iid]), 'Invalid event data provided'])
                        return False, err_msg, None
                except KeyError:
                    if algo_conf['algorithm_name'] == 'rec':
                        err_msg.extend(['The event type: \'{e}\' is currently set as this accounts recommendations event type. Please supply the rec_item_identifier key. ex. \'item_id\' with a relevant value within the event_properties.'.format(e=event_type_id), 'Invalid event data provided'])
                        return False, err_msg, None
        return True, None, event_type_id

    #Delete all events for an account
    async def delete_account_events_internal(self) -> bool:
        """
        Returns
        ----------
        result : bool
        """

        try:
            await eventsRepository.delete_account_events(self.account_id)
            await eventTypesRepository.delete_account_event_types(self.account_id)
            return True
        except Exception as x:
            raise OctyException(500, 'Server error', [{'error_message' : 'An error occurred while attempting to delete events for this account. Please try again later.', 'extended_help': ''}])
            

    async def get_events(self, timeframe : int, cursor : int, event_sequence_event : dict = None, profile_ids : list = None, event_type : str = None) -> Union[list, int]:
        """
        Parameters
        ----------
        timeframe : int
            the number of minutes events must have occurred in, since now
        cursor : int
            pagination cursor
        event_sequence_event : dict
            past segment definition event sequence event object

        Returns
        ----------
        events : list
        total : int
        """
        events, total = await eventsRepository.get_events(self.account_id, timeframe, cursor, event_sequence_event, profile_ids, event_type)
        if len(events) < 1:
            raise OctyException(400, 'No events found', 
                [{'error_message' : 'No events found with provided params or pagination cursor exhausted', 
                'extended_help': ''}])
        return events, total
