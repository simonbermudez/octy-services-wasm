# module imports
from data.repositories.implementation.segmentation_repository import segmentationRepository
from .billing import BillingUnits
from utils.utils import *
from config import Config

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import logging
import sys
import copy
import time

# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
import strconv
from sentry_sdk import capture_exception


class PastSegmentation():
    """
        PastSegmentation
        Handles:
        - Past segmentation
        ...
    """
    def __init__(self, account_id : str, account_type : str, account_currency : str, webhook_url : str, octy_job_id : str, segment_id : str, loop : Any):
        self.account_id = account_id
        self.webhook_url = webhook_url
        self.octy_job_id = octy_job_id
        self.segment_id = segment_id
        self.loop = loop
        self.b = BillingUnits(account_id=account_id, account_type=account_type, account_currency=account_currency, process_name='past_segmentation', loop=loop)
        self.gsdo = {"account_id": self.account_id, "operations" :[]} # Grouped Segmentation Database Operations
        self.gsdo_size_limit = 104857600 #100 MB AMQP message limit
        self.logger = logging.getLogger('uvicorn.error')
        self.matching_profile_ids = []
        self.segment = None

    async def _add_sdo(self, operation : dict):
        if sys.getsizeof(self.gsdo['operations']) > self.gsdo_size_limit:
            self.gsdo['operations'].append(operation)
            await self._process_gsdo()
        else:
            self.gsdo['operations'].append(operation)
    
    async def _process_gsdo(self): 
        # Process all grouped messages
        if len(self.gsdo['operations']) > 0:
            self.loop.create_task(amqpPublisher.send_message(routing_key='grouped.segmentation.operations.cmd',
                payload=self.gsdo))
            # flush grouped messages
            self.gsdo = {"account_id": self.account_id, "operations" :[]}

    async def _exit_segmentation_process(self, message : str, status : str = 'success' , segments_summary : list = None) -> None:
        try:
            self.logger.info(str(message) + " -- " + str(dt.now()))
            self.logger.info(segments_summary)
            
            # Process any messages 
            await self._process_gsdo()

            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : str(message),
                'status' : status
            })
            self.b.complete_compute_units()
        except Exception as err:
            capture_exception(err)
            self.b.complete_compute_units()
            self.logger.critical(f'Error occurred when attempting to exit segmentation process. {str(err)}')
    
    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                headers={'cursor': str(0)},
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0} seconds')
    
    async def _update_tag(self, tag : dict, profile_id : str, status : str) -> None:
        self.logger.info(f"Updating tag with Segment ID : {tag['segment_id']} to '{status}'")
        await self._add_sdo({
                'action' : 'update',
                'operation_payload' : {
                    'profile_id' : profile_id,
                    'segment_tags' : [
                        {
                            'segment_id' : tag['segment_id'],
                            'segment_tag' : tag['segment_tag'],
                            'status' : status
                        }
                    ]
                }
        })
    
    async def _create_tag(self, tag : dict, profile_id : str, status : str = 'pending') -> None:
        self.logger.info(f"Creating new segment tag for segment with Segment ID : {tag['segment_id']}")
        await self._add_sdo({
            'action' : 'create',
            'operation_payload' : {
                'profile_id' : profile_id,
                'segment_tags' : [
                    {
                        'segment_id' : tag['segment_id'],
                        'segment_tag' : tag['segment_name'],
                        'status' : status
                    }
                ]
            }
        })

    async def _delete_tag(self, tag : dict, profile_id : str) -> None:
        self.logger.info(f"Deleting tag with Segment ID : {tag['segment_id']} from profile : {profile_id}")
        await self._add_sdo({
            'action' : 'delete',
            'operation_payload' : {
                'profile_id' : profile_id,
                'segment_tags' : [
                    {
                        'segment_id' : tag['segment_id']
                    }
                ]
            }
        })

    async def _delete_past_segment_tag(self, profile : dict) -> bool:
        try:
            self.logger.info("Should be deleting tag -- " + str(dt.now()))
            #determine if segment tag exists in profiles segment tags arrary
            tags = list(filter(lambda st : st["segment_id"] == self.segment['segment_id'] and (st["status"] == 'active' or st["status"] == 'pending'), profile['segment_tags']))
            if len(tags)>0:

                for tag in tags:
                    #if a tag exists, switch through status
                    if tag['status'] == 'active': 
                        # IF a pending deletion tag already exists for THIS tag, delete currently active tag instead 
                        # of updating to 'pending deletion'. There should only ever be one pending deletion tag, 
                        # for any given segment at any given time.

                        # Delete existing ACTIVE tag
                        await self._delete_tag(tag, profile['profile_id'])

                    elif tag['status'] == 'pending':
                        # Delete existing PENDING tag 
                        await self._delete_tag(tag, profile['profile_id'])
                        # self.logger.info("Updating THIS tag to 'inactive'")
                        # await self._update_tag(tag, profile['profile_id'],'inactive')
            else:
                self.logger.info("No tag found for segment and profile.. skipping")
            return True
        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))
            return False
    
    async def _create_past_segment_tag(self, profile : dict , status : str = 'pending') -> None:
        try:
            #determine if segment tag exists in profiles segment tags arrary
            tag = next((st for st in profile['segment_tags'] if st["segment_id"] == self.segment['segment_id'] and st["status"] == 'active'), None)
            if not tag:
                #If no tag exists, proceed to create new tag
                await self._create_tag(self.segment, profile['profile_id'], status)
            else:
                self.logger.info(f"Tag with Segment ID : {self.segment['segment_id']} already exists in profile with ID : {profile['profile_id']}")
        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))

    async def _property_evaluation(self, profile : object, profile_property_name : str, profile_property_value : any) -> bool:
        if profile_property_name == None or profile_property_value == None:
            return False
        # check if profile_property_name key exists
        try:
            #infer data type for profile_property_value from stored string value
            profile_data = profile['profile_data']
            property_ = profile_data[profile_property_name]
        except KeyError:
            self.logger.warning(f"Key error occurred, profile_property_name : {profile_property_name} not in this profile_data \
                ::ERROR:: -- profile_id : {profile['profile_id']} . -- {str(dt.now())}")
            return False
        #convert property to inferred type to compare
        try:
            profile_property_value_inferred = strconv.convert(profile_property_value)
        except Exception:
            self.logger.error("Conversion error occurred when inferring 'profile_property_value' data type")

        if property_ != profile_property_value_inferred:
            self.logger.info(f"Profile with ID: {profile['profile_id']} did not match this segments required profile property value")
            return False

        self.logger.info(f"Profile with ID: {profile['profile_id']} matched this segments required profile property value")
        return True

    async def _get_profiles(self, past_events : list) -> list:
        # Get all profile_id(s) that previously met this segments criteria 
        # and combine them with the profile_ids found in retrieved past events.
        profile_ids = self.segment['profile_ids']
        profile_ids.extend([ev_profile["profile_id"] for ev_profile in past_events])
        #de duplicate profile_ids
        profile_ids= list(set(profile_ids))
        profiles = await segmentationRepository.get_profiles_by_id(self.account_id, profile_ids)
        if len(profiles)<1:
            raise Exception(f'No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {self.account_id}')
        return profiles

    async def _filter_profile_events(self, events : list, profile_id : str) -> list:
        return list(filter(lambda x : x['profile_id'] == profile_id, events))

    async def _event_sequence_analysis(self, past_events : list) -> Union[dict, bool]:
        invalid_event_sequence = True
        events_map={}

        for event_sequence_event in self.segment['event_sequence']:
            events_map[event_sequence_event['event_type']]={
                'found' : False,
                'action_inaction' : event_sequence_event['action_inaction'],
                'time_stamp' : None
            }

        for event_sequence_event in self.segment['event_sequence']:
            seg_events_prop_map={}
            for event in past_events:
                '''
                    If no event_properties keys supplied inside event_sequence_event['event_properties'], 
                    We should not try and compare event properties, this is assumed the client wants to segment on event type alone.

                    Alternativley, if we have keys and values in seg_event['event_properties'], we should determine if the key:value pairs exist IN the 
                    event['event_properties'], not all event['event_properties'] key:value pairs need to present in seg_event['event_properties'] to qualify. 
                '''
                if event['event_type'] == event_sequence_event['event_type']:

                    #CASE ONE: event_properties not supplied in segment definition :: PASS
                    if event_sequence_event['event_properties'] == None:
                        invalid_event_sequence = False
                        events_map[event_sequence_event['event_type']]['found'] = True
                        events_map[event_sequence_event['event_type']]['time_stamp'] = str_to_dt(event['created_at'])

                    else:
                        #get key value from event_properties
                        for k, v in event_sequence_event['event_properties'].items():
                            #Add keys to seg_events_prop_map, with the value False. 
                            seg_events_prop_map[k]=False #Overwrite if true for this key as both properties must be present in the same event

                            #check if current k, v (key:value) exist in current event.event_properties, if not, current profile does not meet criteria for this segment. 
                            try:
                                event_dict = event['event_properties']
                                if event_dict[k] == v:
                                    seg_events_prop_map[k]=True
                            except KeyError:
                                continue
                                #key does not exist in current event.event_properties
                
                    #CASE TWO: event_properties supplied, all provided key pair values supplied in seg event properties were found 
                    # in one or more of the profiles past events :: PASSED

                    #CASE THREE: event_properties supplied, NOT all provided key pair values supplied in seg event properties were found 
                    # in one or more of the profiles past events :: FAILED
                    invalid_event_props=False
                    for _, found in seg_events_prop_map.items():
                        if found == False:
                            invalid_event_props=True
                            break

                    if not invalid_event_props:
                        if events_map[event_sequence_event['event_type']]['found'] == True:
                            break
                        invalid_event_sequence = False
                        events_map[event_sequence_event['event_type']]['found'] = True
                        events_map[event_sequence_event['event_type']]['time_stamp'] = str_to_dt(event['created_at'])
        
        return events_map, invalid_event_sequence

    async def _event_map_analysis(self, events_map : dict) -> bool:
        meets_criteria=True
        for k, _ in events_map.items():
            if events_map[k]['action_inaction'] == 'inaction':
                if events_map[k]['found']==True:
                    meets_criteria=False
            elif events_map[k]['action_inaction'] == 'action':
                if events_map[k]['found']==False:
                    meets_criteria=False
        return meets_criteria

    async def _get_segment(self):
        segments = await segmentationRepository.get_segment_definitions(account_id=self.account_id,segment_id=self.segment_id)
        if len(segments)<1:
            raise Exception(f'No segment found with ID : {self.segment_id}')
        
        self.segment = segments[0]

    async def run(self) -> None:
        try:
            self.b.track_compute_units('hours')
            await self._get_segment()
            segment_customer_count = 0

            self.logger.info("Past segmentation -- " + str(dt.now()))
            self.logger.info(f"Segment_id: {self.segment_id} {str(dt.now())}")
            self.logger.info(f"Segment name: {self.segment['segment_name']} {str(dt.now())}")

            # Get events that match segment event sequence events
            found_past_events = []
            for ev in self.segment['event_sequence']:
                segment_timeframe_minutes = (self.segment['segment_timeframe']*24) * 60 # Convert segment timeframe days to minutes 
                events = await segmentationRepository.get_events(self.account_id, segment_timeframe_minutes, ev)
                found_past_events.extend(events)
            
            profiles = await self._get_profiles(found_past_events)
            for profile in profiles: 
                # Determine if active segment tag exists. If no events are found, 
                # its assumed that this profile no longer meets this segments criteria.
                active_tag_exists = False
                if next((st for st in profile['segment_tags'] if st["segment_id"] == self.segment['segment_id'] and st["status"] == 'active'), None): 
                    active_tag_exists = True
                
                print("========================================")
                self.logger.info(f"Processing profile with ID: {profile['profile_id']}")
                
                # Filter events conducted by this profile
                past_events = await self._filter_profile_events(found_past_events, profile['profile_id'])
                
                events_map, invalid_event_sequence = await self._event_sequence_analysis(past_events)
                if invalid_event_sequence: 
                    # Invalidate tag where required event occurred prior to timeframe
                    if active_tag_exists:
                        await self._delete_past_segment_tag(profile=profile)
                    continue

                meets_criteria = await self._event_map_analysis(events_map)
                if meets_criteria:
                    #evaluate profile properties if specified subtype
                    if self.segment['segment_sub_type'] == 3 or self.segment['segment_sub_type'] == 4:
                        self.logger.info(f"Profile : {profile['profile_id'] + str(dt.now())} met this segments criteria based on events.. assessing profile properties ")
                        evaluate = await self._property_evaluation(profile, self.segment['profile_property_name'], self.segment['profile_property_value'])
                        if evaluate == False:
                            await self._delete_past_segment_tag(profile=profile)
                            continue
                    
                    self.matching_profile_ids.append(profile['profile_id'])
                    await self._create_past_segment_tag(profile=profile, status='active')
                    segment_customer_count +=1
                else:
                    await self._delete_past_segment_tag(profile=profile)
                    continue


            #update segments matching_profile_ids
            await segmentationRepository\
                .update_segment_profiles_ids(self.account_id, self.segment['segment_id'], self.matching_profile_ids)


            msg = 'Segmentation proccess complete.'
            if segment_customer_count <1:
                msg = 'Segmentation proccess complete. No customer events currently meet this segments criteria.'
            segments_summary = [{
                    'segment_id' : self.segment['segment_id'],
                    'segment_name' : self.segment['segment_name'],
                    'segment_type' : self.segment['segment_type'],
                    'count' : segment_customer_count
                }]

            await self._exit_segmentation_process(message=msg, segments_summary=segments_summary)

        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))
            await self._exit_segmentation_process(message='Past segmentation exitied early due to an error.', status='failed')


class LiveSegmentation():
    """
        LiveSegmentation
        Handles:
        - Live segmentation
        ...

        Parameters
        ----------
        account_id : str
        webhook_url : str
        octy_job_id : str
        event_obj : object
    """
    def __init__(self, account_id : str, webhook_url : str, octy_job_id : str, event_obj : object, loop : Any):
        self.account_id = account_id
        self.webhook_url = webhook_url
        self.octy_job_id = octy_job_id
        self.event = event_obj.dict() # parse event object to dict
        self.loop = loop
        self.gsdo = {"account_id": self.account_id, "operations" :[]} # Grouped Segmentation Database Operations
        self.gsdo_size_limit = 104857600 #100 MB AMQP message limit
        self.logger = logging.getLogger('uvicorn.error')
        self.live_validation_octy_job_time_buffer = 2 # number of minutes to add to live_validation_octy_job timeframe
        self.es_event_property_map = {}
        self.profile = None

    async def _add_sdo(self, operation : dict):
        if sys.getsizeof(self.gsdo['operations']) > self.gsdo_size_limit:
            self.gsdo['operations'].append(operation)
            await self._process_gsdo()
        else:
            self.gsdo['operations'].append(operation)
    
    async def _process_gsdo(self): 
        # Process all grouped messages
        if len(self.gsdo['operations']) > 0:
            self.loop.create_task(amqpPublisher.send_message(routing_key='grouped.segmentation.operations.cmd',
                payload=self.gsdo))
            # flush grouped messages
            self.gsdo = {"account_id": self.account_id, "operations" :[]}

    async def _exit_segmentation_process(self, message : str, status : str = 'success') -> None:
        try:
            self.logger.info(str(message) + " -- " + str(dt.now()))
            
            # Process any messages 
            await self._process_gsdo()

            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : str(message),
                'status' : status
            })
        except Exception as err:
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to exit segmentation process. {str(err)}')
        
    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0} seconds')

    async def _send_http_account_webhook_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            self.logger.error('Exception', x.__class__.__name__)
            self.logger.error(f'Error: {x}')
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0} seconds')
    
    async def _send_webhook_request(self, segment : dict) -> None:
        self.logger.info("Sending webhook [new Live segment tag created] -- " + str(dt.now()))
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Live segment tag created',
            'body' : {
                'profile_id' : self.profile['profile_id'],
                'segment' : {
                    'segment_id' : segment['segment_id'],
                    'segment_tag' : segment['segment_name']
                }
            },
            'date_time' : str(dt.now())
        })

    async def _update_tag(self, tag : dict, profile_id : str, status : str) -> None:
        self.logger.info(f"Updating tag with Segment ID : {tag['segment_id']} to '{status}'")
        await self._add_sdo({
                'action' : 'update',
                'operation_payload' : {
                    'profile_id' : profile_id,
                    'segment_tags' : [
                        {
                            'segment_id' : tag['segment_id'],
                            'segment_tag' : tag['segment_tag'],
                            'status' : status
                        }
                    ]
                }
        })
    
    async def _create_tag(self, tag : dict, profile_id : str, status : str = 'pending') -> None:
        self.logger.info(f"Creating new segment tag for segment with Segment ID : {tag['segment_id']}")
        await self._add_sdo({
            'action' : 'create',
            'operation_payload' : {
                'profile_id' : profile_id,
                'segment_tags' : [
                    {
                        'segment_id' : tag['segment_id'],
                        'segment_tag' : tag['segment_name'],
                        'status' : status
                    }
                ]
            }
        })

    async def _delete_tag(self, tag : dict, profile_id : str) -> None:
        self.logger.info(f"Deleting tag with Segment ID : {tag['segment_id']} from profile : {profile_id}")
        await self._add_sdo({
            'action' : 'delete',
            'operation_payload' : {
                'profile_id' : profile_id,
                'segment_tags' : [
                    {
                        'segment_id' : tag['segment_id']
                    }
                ]
            }
        })
    
    async def _delete_live_segment_tag(self) -> bool:
        try:
            self.logger.info("Should be deleting tag -- " + str(dt.now()))
            #determine if segment tag exists in profiles segment tags arrary
            tags = list(filter(lambda st : st["segment_id"] == self.segment['segment_id'] and (st["status"] == 'active' or st["status"] == 'pending'), self.profile['segment_tags']))
            if len(tags)>0:

                for tag in tags:
            
                    #if a tag exists, switch through status
                    if tag['status'] == 'active': 
                        # Delete existing ACTIVE tag
                        await self._delete_tag(tag, self.profile['profile_id'])

                    elif tag['status'] == 'pending': 
                        # self.logger.info("Updating THIS tag to 'inactive'")
                        # await self._update_tag(tag, self.profile['profile_id'],'inactive')

                        # Delete existing PENDING tag
                        await self._delete_tag(tag, self.profile['profile_id'])
            else:
                self.logger.info("No tag found for segment and profile.. skipping")
            return True
        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))
            return False

    async def _create_live_segment_tag(self, segment : dict, status : str = 'pending') -> Union[bool, str]:
        try:
            #determine if segment tag exists in profiles segment tags arrary
            tags = list(filter(lambda st : st["segment_id"] == segment['segment_id'] and (st["status"] == 'active' or st["status"] == 'pending'), self.profile['segment_tags']))
            if len(tags)>0:
                for tag in tags:
                    #if a tag exists, switch through status to determine if new tag should be created
                    if tag['status'] == 'active': 
                        #If segment is live, and this tag exists, use this method as a change agent to keep live 
                        # segment tags up to date. Also, this will allow webhook to sent EACH TIME profile meets 
                        # segment definition. 

                        # Delete currently active tag and create new active tag. There should only ever be one active tag, 
                        # for any given segment at any given time.
                        self.logger.info(f"Active tag exists for segment with ID: {segment['segment_id']}. Deleting existing active tag")
                        await self._delete_tag(tag, self.profile['profile_id'])

                        # create new segment tag
                        await self._create_tag(segment, self.profile['profile_id'], status)
                        #self.logger.info(f"New pending tag created for segment with ID: {segment['segment_id']}")
                        return True, 'new'

                    elif tag['status'] == 'pending':
                        self.logger.warning(f"Tag already exists or segment with ID: {segment['segment_id']}, skipping...")
                        return True, tag['status']
            else:
                # create new segment tag
                await self._create_tag(segment, self.profile['profile_id'], status)
                return True, 'new'

        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))
            return False, None

    async def _update_live_segment_tag(self, segment : dict, status : str) -> None:
        # Update if segment tag is pending, else pass and just send webhook
        tag = next((st for st in self.profile['segment_tags'] if st["segment_id"] == segment['segment_id'] and st["status"] == 'pending'), None)
        if tag:
            await self._update_tag(tag, self.profile['profile_id'],status)
            
            if status == 'active':
                #Send webhook notification of new segment tag
                self.logger.info("Sending webhook [new Live segment tag created] -- " + str(dt.now()))
                await self._send_http_request(self.webhook_url, {
                    'subject' : 'Live segment tag created',
                    'body' : {
                        'profile_id' : self.profile['profile_id'],
                        'segment' : {
                            'segment_id' : tag['segment_id'],
                            'segment_tag' : tag['segment_name']
                        }
                    },
                    'date_time' : str(dt.now())
                })

    async def _create_live_validation_octy_job(self, segment_event : dict, segment_id : str) -> None:
        time_interval = segment_event['exp_timeframe']+self.live_validation_octy_job_time_buffer
        self.loop.create_task(amqpPublisher.send_message(routing_key='octy.job.cmd.create',
            payload={
                'account_id' : self.account_id,
                'job_meta' : {
                    'job_type' : 'pending-live',
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
                    'time_interval' : time_interval,
                    'fail_threshold' : 10
                },
                'job_data' : {
                        'segment_data' : {
                            'segmentation_type' : 'pending-live',
                            'segment_id' : segment_id
                        },
                        'event_data' : {
                            'profile_id' : self.profile['profile_id'],
                            'event_timeframe' : 5
                        },
                        'validation_job' : True,
                        'live_octy_job_id' : self.octy_job_id
                }
        }))
        self.logger.info(f"Creating new octy-job for this profile and segment with a time interval of {str(time_interval)} minutes")

    async def _get_profile(self):
        profiles = await segmentationRepository.get_profiles_by_id(self.account_id, [self.event['profile']['profile_id']])
        if len(profiles)<1:
            raise Exception(f'No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {self.account_id}')
        self.profile = profiles[0]

    async def _event_sequence_event_property_analysis(self, event_sequence_event : dict) -> bool:
        invalid_event_sequence_event=False
        
        if event_sequence_event['event_properties'] == None:
            self.logger.info("No event properties required for this event sequence event")
            #CASE ONE: event_properties not supplied. Profile meets the criteria for this event sequence event
            return invalid_event_sequence_event

        else:
            self.logger.info("Event properties ARE required for this event sequence event. Assessing provided event properties...")
            #get key value from event_properties
            for k, v in event_sequence_event['event_properties'].items():
                #Add keys to seg_events_prop_map, with the value False. 
                self.es_event_property_map[k]=False #Overwrite if true for this key as all properties must be present in the same event for it be a valid event.

                #check if current k, v (key:value) exist in current event.event_properties, if not, current profile does not meet criteria for this segment. 
                try:
                    event_dict = self.event['event_properties']
                    if event_dict[k] == v:
                        self.es_event_property_map[k]=True
                except KeyError:
                    self.logger.warning(f"Required key {k} not present in event event properties, continuing...")
                    continue
                    #key does not exist in current event.event_properties

            return invalid_event_sequence_event

    async def _es_event_property_map_analysis(self) -> bool:
        #CASE TWO: event_properties supplied, all provided key pair values supplied in seg event properties were found 
            # in one or more of the profiles past events :: PASSED

        #CASE THREE: event_properties supplied, NOT all provided key pair values supplied in seg event properties were found 
        # in one or more of the profiles past events :: FAILED
        invalid_event_properties = False
        for _, found in self.es_event_property_map.items():
            if found == False:
                invalid_event_properties=True
                break
        return invalid_event_properties

    async def _segment_type_one_analysis(self, segment : dict) -> None:
        for event_sequence_event in segment['event_sequence']:
            # Reset map for each event
            self.es_event_property_map = {}

            if self.event['event_type'] != event_sequence_event['event_type']:
                self.logger.info("Skipping...")
                return
            
            # analyse each event_sequence_event
            invalid_event_sequence_event = await self._event_sequence_event_property_analysis(event_sequence_event)
            if invalid_event_sequence_event:
                # if any invlaid, return 
                self.logger.info("Skipping... Invalid event sequence > event")
                return

            # analyse event properties map to ensure all event properties were provided
            invalid_event_properties = await self._es_event_property_map_analysis()
            if not invalid_event_properties:
                res, _ = await self._create_live_segment_tag(segment=segment, status='active')
                if not res:
                    raise Exception('Unexpected error occurred when attempting to create segment tag')
                await self._send_webhook_request(segment)
            self.logger.info("Skipping... Invalid event sequence > event > event properties")
            
    async def _segment_type_two_analysis(self, segment : dict) -> None:

        for event_sequence_event in segment['event_sequence']:
            # Reset map for each event
            self.es_event_property_map = {}

            if self.event['event_type'] == event_sequence_event['event_type'] \
                and event_sequence_event['action_inaction'] == 'action':
                
                # analyse each event_sequence_event
                invalid_event_sequence_event = await self._event_sequence_event_property_analysis(event_sequence_event)
                if invalid_event_sequence_event:
                    # if any invlaid, return 
                    self.logger.info("Skipping... Invalid event sequence event")
                    return

                # analyse event properties map to ensure all event properties were provided
                invalid_event_properties = await self._es_event_property_map_analysis()
                if not invalid_event_properties:
                    res, status = await self._create_live_segment_tag(segment=segment, status='pending')
                    if not res:
                        raise Exception('Unexpected error occurred when attempting to create segment tag')
                    
                    #Ensure if octy-job has already been created for profile & tag combo a second one is not created
                    if status == 'pending':
                        self.logger.info("Octy job exists for this profile and segment.. continuing to next live segment definition")
                        return
                    await self._create_live_validation_octy_job(event_sequence_event, segment['segment_id'])

                else:
                    self.logger.info("Skipping... Invalid event sequence > event > event properties")

    async def _segment_type_two_pending_tag_analysis(self, segment : dict, pending_tag : dict) -> None:
        #we have the initial action the segment event sequence, now check the existence and validity of the subsequent inaction.
        #init copy of event list
        events_sequence_copy = copy.deepcopy(segment['event_sequence'])
        #access previous action -- asserting that the previous event is an action and that it is the first event in the event sequence
        previous_event = events_sequence_copy[0]
        if previous_event['action_inaction'] == 'inaction':
            #Prevent the outside case of the first element in event sequence being an inaction
            return

        #access time stamp from remaining element in list. MINUTES
        previous_event_timeframe = previous_event['exp_timeframe']
        #if event occurred after time frame update tag status as 'active'
        deadline = int_to_dt(pending_tag['created_at']['$date'], False) + td(minutes=previous_event_timeframe)

        if dt.now() > deadline:
            # CASE ONE: Timeframe has passed -- Does meet criteria -- VALID
            '''
                Initial 'action' event has occurred and second event 'inaction' did not occur within the time frame. It is now impossible for this segments criteria to not be met.
                Example:
                    Watch video (action) : Found,
                    Page view (inaction) : Not found and passed deadline
            '''
            #UPDATE TAG TO ACTIVE
            await self._update_live_segment_tag(segment, 'active')

        elif dt.now() <= deadline:
            for event_sequence_event in segment['event_sequence']:
                # Reset map for each event
                self.es_event_property_map = {}
                #We are pre deadline, therefore we only need to check if the if the inaction has occurred
                # CASE TWO: Timeframe has NOT passed AND 'inactive' event occurred, if required event properties found -- Does not meet criteria -- INVALID
                if self.event['event_type'] == event_sequence_event['event_type'] \
                    and event_sequence_event['action_inaction'] == 'inaction':
                    
                    # analyse each event_sequence_event
                    invalid_event_sequence_event = await self._event_sequence_event_property_analysis(event_sequence_event)
                    if invalid_event_sequence_event:
                        # if any invlaid, return 
                        self.logger.info("Skipping... Invalid event sequence event")
                        return
                    
                    # analyse event properties map to ensure all event properties were provided
                    invalid_event_properties = await self._es_event_property_map_analysis()
                    if not invalid_event_properties:
                        #Found valid Inaction event -- DELETE PENDING TAG
                        await self._delete_live_segment_tag()

    async def run(self) -> None:
        try:
            await self._get_profile()

            live_segments = await segmentationRepository.get_segment_definitions(self.account_id, 'live')
            if len(live_segments)<1:
                raise Exception(f'No LIVE segments associated with this account. Account ID : {self.account_id}')

            for segment in live_segments:
                print("==============================================")
                self.logger.info(f"Analysing live segment : {segment['segment_id']}")
                self.logger.info("Analysing segment event sequence ...")

                if segment['segment_sub_type'] == 1: # Single action, # Should only be one event in event sequence
                    self.logger.info("Segment sub-type 1")
                    await self._segment_type_one_analysis(segment)

                elif segment['segment_sub_type'] == 2: #Single action followed by a single inaction

                    self.logger.info("Segment sub-type 2")
                    # determine if segment tag created with this segment_id and status of 'pending'. 
                    # 'pending' status means the action (first event) in a sub type 2 has been performed.
                    is_pending_tag = next((st for st in self.profile['segment_tags'] \
                        if st["segment_id"] == segment['segment_id'] and st["status"] == 'pending'), None)
                    
                    if not is_pending_tag:
                        #IF we have no pending tag for this segment, 
                        # check the condition of the current event to see if it matches segment event sequence criteria
                        self.logger.info(f"No pending tag found for segment: {segment['segment_id']} and profile : {self.profile['profile_id']}")
                        await self._segment_type_two_analysis(segment)
                    else:
                        self.logger.info(f"Pending tag FOUND for segment: {segment['segment_id']} and profile : {self.profile['profile_id']}")
                        #NOTE: If analysisng sub type 2 segments here and in pending live job causes duplicates, 
                        # simply comment out the below method call.
                        # For now, we can leave the analysis of pending tags to relative pending live jobs as one will exist 
                        # for this pending segment tag already.

                        #await self._segment_type_two_pending_tag_analysis(segment, is_pending_tag)

            await self._exit_segmentation_process(message='Live segmentation job complete')
        except Exception as e:
            capture_exception(e)
            await self._exit_segmentation_process(message=str(e), status='failed')


class PendingLiveSegmentation():
    """
        PendingLiveSegmentation
            We can assert that the dealine has passed because this process will only be run beyond the deadline.
            Get all events for profile from the last x minutes to be certain the event has no occurred.
            If no matching 'inaction' event found, update pending tag to 'active', else delete tag.
        ...

        Parameters
        ----------
        account_id : str
        segment_id : str
        profile_id : str
        octy_job_id : str
        live_octy_job_id : str
        event_timeframe : int 
    """

    def __init__(self, account_id : str, webhook_url : str, segment_id : str, profile_id : str, octy_job_id : str, live_octy_job_id : str, event_timeframe : int, loop : Any):
        self.account_id = account_id
        self.webhook_url = webhook_url
        self.segment_id = segment_id
        self.profile_id = profile_id
        self.octy_job_id = octy_job_id
        self.live_octy_job_id  = live_octy_job_id
        self.event_timeframe = event_timeframe + 1
        self.loop = loop
        self.gsdo = {"account_id": self.account_id, "operations" :[]} # Grouped Segmentation Database Operations
        self.gsdo_size_limit = 104857600 #100 MB AMQP message limit
        self.logger = logging.getLogger('uvicorn.error')
        self.profile = None
        self.segment = None
        self.found_past_inaction_events = []
    
    async def _add_sdo(self, operation : dict):
        if sys.getsizeof(self.gsdo['operations']) > self.gsdo_size_limit:
            self.gsdo['operations'].append(operation)
            await self._process_gsdo()
        else:
            self.gsdo['operations'].append(operation)
    
    async def _process_gsdo(self): 
        # Process all grouped messages
        if len(self.gsdo['operations']) > 0:
            self.loop.create_task(amqpPublisher.send_message(routing_key='grouped.segmentation.operations.cmd',
                payload=self.gsdo))
            # flush grouped messages
            self.gsdo = {"account_id": self.account_id, "operations" :[]}

    async def _exit_segmentation_process(self, message : str, status : str = 'success') -> None:
        try:
            self.logger.info(str(message) + " -- " + str(dt.now()))
            
            # Process any messages 
            await self._process_gsdo()

            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : str(message),
                'status' : status
            })
        except Exception as err:
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to exit segmentation process. {str(err)}')
    
    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0} seconds')

    async def _send_http_account_webhook_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                timeout=60, 
                data=json.dumps(payload)
            )
        except Exception as x:
            self.logger.error('Exception', x.__class__.__name__)
            self.logger.error(f'Error: {x}')
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0} seconds')
    
    async def _send_webhook_request(self, segment : dict) -> None:
        self.logger.info("Sending webhook [new Live segment tag created] -- " + str(dt.now()))
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Live segment tag created',
            'body' : {
                'profile_id' : self.profile['profile_id'],
                'segment' : {
                    'segment_id' : segment['segment_id'],
                    'segment_tag' : segment['segment_name']
                }
            },
            'date_time' : str(dt.now())
        })

    async def _update_tag(self, profile_id : str, tag : dict, status : str) -> None:
        self.logger.info(f"Updating tag with Segment ID : {tag['segment_id']} to '{status}'")
        await self._add_sdo({
                'action' : 'update',
                'operation_payload' : {
                    'profile_id' : profile_id,
                    'segment_tags' : [
                        {
                            'segment_id' : tag['segment_id'],
                            'segment_tag' : tag['segment_tag'],
                            'status' : status
                        }
                    ]
                }
        })
    
    async def _delete_tag(self, tag : dict, profile_id : str) -> None:
        self.logger.info(f"Deleting tag with Segment ID : {tag['segment_id']} from profile : {profile_id}")
        await self._add_sdo({
            'action' : 'delete',
            'operation_payload' : {
                'profile_id' : profile_id,
                'segment_tags' : [
                    {
                        'segment_id' : tag['segment_id']
                    }
                ]
            }
        })

    async def _update_live_segment_tag(self, status : str) -> None:
        # Update if segment tag is pending, else pass and just send webhook
        tags = list(filter(lambda st : st["segment_id"] == self.segment['segment_id'] and (st["status"] == 'active' or st["status"] == 'pending'), self.profile['segment_tags']))
        if len(tags)>0:

            for tag in tags:
                if tag['status'] == 'active' and status == 'active':
                    pass # if active tag already exists, skip
                else:
                    await self._update_tag(self.profile['profile_id'], tag,status)

                if status == 'active':
                    #Send webhook notification of new segment tag
                    self.logger.info("Sending webhook [new Live segment tag created] -- " + str(dt.now()))
                    await self._send_http_request(self.webhook_url, {
                        'subject' : 'Live segment tag created',
                        'body' : {
                            'profile_id' : self.profile['profile_id'],
                            'segment' : {
                                'segment_id' : tag['segment_id'],
                                'segment_tag' : tag['segment_tag']
                            }
                        },
                        'date_time' : str(dt.now())
                    })
    
    async def _delete_live_segment_tag(self) -> bool:
        try:
            self.logger.info("Should be deleting tag -- " + str(dt.now()))
            #determine if segment tag exists in profiles segment tags arrary
            tags = list(filter(lambda st : st["segment_id"] == self.segment['segment_id'] and (st["status"] == 'active' or st["status"] == 'pending'), self.profile['segment_tags']))
            if len(tags)>0:

                for tag in tags:
            
                    #if a tag exists, switch through status
                    if tag['status'] == 'active': 
                        # Delete existing ACTIVE tag
                        await self._delete_tag(tag, self.profile['profile_id'])

                    elif tag['status'] == 'pending': 
                        # self.logger.info("Updating THIS tag to 'inactive'")
                        # await self._update_tag(tag, self.profile['profile_id'],'inactive')

                        # Delete existing PENDING tag
                        await self._delete_tag(tag, self.profile['profile_id'])
            else:
                self.logger.info("No tag found for segment and profile.. skipping")
            return True
        except Exception as err:
            capture_exception(err)
            self.logger.critical(str(err) + " -- " + str(dt.now()))
            return False

    async def _get_profile(self):
        profiles = await segmentationRepository.get_profiles_by_id(self.account_id, [self.profile_id])
        if len(profiles)<1:
            raise Exception(f'No profiles associated with this account, or existing profiles have conducted no event instances. Account ID : {self.account_id}')
        self.profile = profiles[0]

    async def _get_segment(self):
        live_segments = await segmentationRepository.get_segment_definitions(self.account_id, segment_id=self.segment_id)
        if len(live_segments)<1:
            raise Exception(f'No LIVE segments associated with this account. Account ID : {self.account_id}')
        self.segment = live_segments[0]
        
    async def _get_past_inaction_events(self): 
        # GET events for profile from the last x minutes (timeframe) and check if inaction event occurred.
        for ev in self.segment['event_sequence']:
            if ev['action_inaction'] == 'inaction': # only get 'inaction' event instances
                events = await segmentationRepository.get_events(self.account_id, self.event_timeframe, ev, profile_ids=[self.profile_id])
                self.found_past_inaction_events.extend(events)

    async def _event_sequence_analysis(self) -> Union[dict, bool]:
        invalid_event_sequence = True
        events_map={}

        for event_sequence_event in self.segment['event_sequence']:
            if event_sequence_event['action_inaction'] != 'inaction':
                continue # Only analyse inaction events
            events_map[event_sequence_event['event_type']]={
                'found' : False,
                'action_inaction' : event_sequence_event['action_inaction'],
                'time_stamp' : None
            }

        for event_sequence_event in self.segment['event_sequence']:
            if event_sequence_event['action_inaction'] != 'inaction':
                continue # Only analyse inaction events
            seg_events_prop_map={}
            for event in self.found_past_inaction_events:
                if event['event_type'] == event_sequence_event['event_type']:

                    #CASE ONE: event_properties not supplied in segment definition :: PASS
                    if event_sequence_event['event_properties'] == None:
                        invalid_event_sequence = False
                        events_map[event_sequence_event['event_type']]['found'] = True
                        events_map[event_sequence_event['event_type']]['time_stamp'] = str_to_dt(event['created_at'])

                    else:
                        #get key value from event_properties
                        for k, v in event_sequence_event['event_properties'].items():
                            #Add keys to seg_events_prop_map, with the value False. 
                            seg_events_prop_map[k]=False #Overwrite if true for this key as both properties must be present in the same event

                            #check if current k, v (key:value) exist in current event.event_properties, if not, current profile does not meet criteria for this segment. 
                            try:
                                event_dict = event['event_properties']
                                if event_dict[k] == v:
                                    seg_events_prop_map[k]=True
                            except KeyError:
                                continue
                                #key does not exist in current event.event_properties
                
                    #CASE TWO: event_properties supplied, all provided key pair values supplied in seg event properties were found 
                    # in one or more of the profiles past events :: PASSED

                    #CASE THREE: event_properties supplied, NOT all provided key pair values supplied in seg event properties were found 
                    # in one or more of the profiles past events :: FAILED
                    invalid_event_props=False
                    for _, found in seg_events_prop_map.items():
                        if found == False:
                            invalid_event_props=True
                            break

                    if not invalid_event_props:
                        if events_map[event_sequence_event['event_type']]['found'] == True:
                            break
                        invalid_event_sequence = False
                        events_map[event_sequence_event['event_type']]['found'] = True
                        events_map[event_sequence_event['event_type']]['time_stamp'] = str_to_dt(event['created_at'])
        
        return events_map, invalid_event_sequence

    async def _event_map_analysis(self, events_map : dict) -> bool:
        meets_criteria=True
        for k, _ in events_map.items():
            if events_map[k]['action_inaction'] == 'inaction':
                if events_map[k]['found']==True:
                    meets_criteria=False
        return meets_criteria

    async def _delete_octy_jobs(self) -> None:
        self.loop.create_task(amqpPublisher.send_message(routing_key='octy.job.cmd.delete',
            payload={
                "account_id" : self.account_id,
                "octy_job_ids" : [self.octy_job_id, self.live_octy_job_id],
                "alt_identifiers" : None
            }))

    async def run(self) -> None:
        try:
            await self._get_profile()
            await self._get_segment()
            await self._get_past_inaction_events()
            
            if len(self.found_past_inaction_events)>0:
                
                events_map, invalid_event_sequence = await self._event_sequence_analysis()
                if invalid_event_sequence:
                    # Update tag to 'active' as no valid inactive events occurred within the defined timeframe
                    await self._update_live_segment_tag('active')
                    await self._delete_octy_jobs()

                meets_criteria = await self._event_map_analysis(events_map)
                if meets_criteria:
                    # Update tag to 'active' as no valid inactive events occurred within the defined timeframe
                    await self._update_live_segment_tag('active')
                    await self._delete_octy_jobs()

                else:
                    # Delete tag as a valid 'inaction' event ocurred within the defined timeframe
                    #await self._update_live_segment_tag('inactive')
                    await self._delete_live_segment_tag()
            else:
                # Update tag to 'active' as no valid inactive events occurred within the defined timeframe
                await self._update_live_segment_tag('active')
                await self._delete_octy_jobs()


            await self._exit_segmentation_process(message='pending live segmentation complete.')
        except Exception as e:
            capture_exception(e)
            await self._exit_segmentation_process(message=str(e), status='failed')