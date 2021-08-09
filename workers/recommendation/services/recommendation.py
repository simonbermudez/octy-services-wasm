# module imports
from data.repositories.implementation.recommendation_repository import recommendationsRepository
from data.repositories.implementation.bucket_repository import bucketRepository
from .AMQP import amqpInterface
from utils.utils import *
from config import Config

# python imports
from typing import *
import json
import time
import logging
import io
import math
import sys
from io import BytesIO
import copy

# external imports
from sentry_sdk import capture_exception
import pandas as pd
import joblib


class RecommenderTraining():

    def __init__(self, account_id : str, octy_job_id : str, bucket : str, algorithm_configurations : dict):
        self.account_id = account_id
        self.octy_job_id = octy_job_id
        self.bucket = bucket
        self.algorithm_configurations = algorithm_configurations
        self.logger = logging.getLogger('uvicorn')
        self.training_job_id = generate_uid('training-job')
        self.data_timeframe = Config['DATA_SET_TIMEFRAME']
        self.profiles_ids = []
        self.features=[{'item_feature_list' : Config['ITEM_FEATURE_COLS']},{'profile_feature_list' : ['rfm_score', 'has_charged']}]
        self.profiles_df = None
        self.events_df = None
        self.items_df = None
        self.csv_objects = []
        self.training_resources = []
        self.total_bytes=0
        self.mpu_upload_id = None
        self.key = None
        self.parts = None

    # async def _clean_dataframe(self, df : object) -> object:
    #     df.dropna(how='any', axis=0, inplace=True)
    #     df.drop_duplicates(keep="first", inplace=True)
    #     return df

    async def _clean_dataframes(self) -> None:

        self.logger.info("Cleaning profiles dataframe...")
        self.profiles_df.dropna(how='any', axis=0, inplace=True)
        #self.profiles_df.drop_duplicates(keep="first", inplace=True)

        self.logger.info("Cleaning events dataframe...")
        self.events_df.dropna(how='any', axis=0, inplace=True)
        #self.events_df.drop_duplicates(keep="first", inplace=True)

        self.logger.info("Cleaning items dataframe...")
        self.items_df.dropna(how='any', axis=0, inplace=True)
        #self.items_df.drop_duplicates(keep="first", inplace=True)
        
    async def _feature_engineering(self) -> None:
        self.logger.info("Conducting feature engineering...")
        profile_cols = list(self.profiles_df.columns)
        system_cols = ['profile_LFM_IDX','profile_id','rfm_score', 'has_charged']
        for c_col in profile_cols:
            if c_col not in system_cols:
                self.features[1]['profile_feature_list'].append(c_col)
        self.logger.info("Completed stage 1 feature engineering...")

        #Merge dataframes
        self.events_df = self.events_df.merge(
            self.profiles_df, how='left',
            left_on='profile_id', right_on='profile_id')
        self.logger.info("Completed stage 2 feature engineering...")

        self.events_df = self.events_df.merge(
            self.items_df, how='left',
            left_on='variable_value', right_on='item_id')
        self.logger.info("Completed stage 3 feature engineering...")
    
        await self._clean_dataframes()
        self.logger.info("Completed stage 4 feature engineering!")

        self.logger.info(self.items_df.info(verbose=True))
        self.logger.info(self.profiles_df.info(verbose=True))
        self.logger.info(self.events_df.info(verbose=True))

        self.logger.info("Completed feature engineering!")


    async def _segment_tag_encoding(self, profile : dict, segment_names : list) -> dict:
        #one hot encode each segment tag
        for seg in segment_names:
            seg_key=seg+'__SEGMENT'
            profile[seg_key]=0
            for seg_tag in profile['segment_tags']:
                if seg_tag['segment_tag'] == seg:
                    profile[seg_key]=1
        return profile

    async def _profile_json_to_dict(self, platform_info : dict, profile_data : dict):
        return_dict={}
        return_dict.update(platform_info)
        return_dict.update(profile_data)
        return return_dict
    
    async def _dataframe_to_csv_obj(self, dataframe : object, type_ : str) -> dict:
        return  {
            'data' : dataframe.to_csv(index=False),
            'type' : type_
        }

    async def _create_csv_objects(self) -> None:
        self.logger.info("Converting datasets to CSV objects...")
        # convert df to csv and store in csv_objects array
        profiles_csv = await self._dataframe_to_csv_obj(self.profiles_df, 'profiles')
        events_csv = await self._dataframe_to_csv_obj(self.events_df, 'events')
        items_csv = await self._dataframe_to_csv_obj(self.items_df, 'items')
        self.csv_objects.extend([profiles_csv, events_csv, items_csv,{
            'data' : json.dumps({'features' : self.features}),
            'type' : 'meta_data'
        }])
        self.logger.info("Converted datasets to CSV objects ready for upload!")

    async def _get_profiles_data(self) -> list: 
        profiles_data = await recommendationsRepository.get_profiles(self.account_id, ids='false')
        return profiles_data

    async def _build_profiles_dataset(self) -> None:
        self.logger.info('Building profiles dataset')
        # pulled from alorithm configurations
        features_list = self.algorithm_configurations['profile_features']
        segments = await recommendationsRepository.get_segments(self.account_id)
        segment_names = []
        for seg in segments:
            segment_names.append(seg['segment_name'])

        profiles = await self._get_profiles_data()
        if len(profiles)< Config['MIN_NUM_PROFILES']:
            raise Exception('Not enough profiles found to conduct model training.')

        profiles_list=[]
        for p in profiles:
            #if RFM score isn't set, default to lowest possible RFM score: 111
            if p['rfm_score'] == '' or p['rfm_score'] == None:
                rfm_score = 111
            else:
                rfm_score = p['rfm_score']
            
            profile_dict={
                'profile_id' : p['profile_id'],
                'has_charged' : p['has_charged'],
                'rfm_score' : rfm_score,
                'segment_tags' : p['segment_tags']
            }

            #deserialize platform info and profile data and convert to columns
            merged_dict = await self._profile_json_to_dict(p['platform_info'], p['profile_data'])
            for k,v in merged_dict.items():
                if k in features_list:
                    profile_dict[k]=v

            profile_dict = await self._segment_tag_encoding(profile_dict, segment_names)
            profiles_list.append(profile_dict)

        self.profiles_df = pd.DataFrame(profiles_list)
        self.profiles_df.dropna(inplace = True)
        #drop columns
        self.profiles_df=self.profiles_df.drop(['segment_tags'], axis=1)
        self.profiles_df.reset_index(drop=True)
        self.profiles_df.insert(0, 'profile_LFM_IDX', range(0, 0+len(self.profiles_df)))

        #Build profile ID list
        self.profiles_ids = self.profiles_df.profile_id.values.tolist()
        

    async def _get_events_data(self) -> list:
        events_data = await recommendationsRepository.get_events(self.account_id, 
                                                                self.profiles_ids, 
                                                                self.data_timeframe, 
                                                                self.algorithm_configurations['event_type'])
        return events_data

    async def _build_events_dataset(self) -> None:
        self.logger.info('Building events dataset')
        events = await self._get_events_data()
        self.logger.info('Got events data from events service')
        if len(events)< Config['MIN_NUM_EVENTS_COLLECTIVE']:
            raise Exception('Not enough events found to conduct model training.')
        
        self.logger.info('Enough events found to conduct model training.')

        events_list=[]
        for event in events:
            event_dict ={
                'profile_id' : event['profile_id'],
                'variable_value': None
            }
            #deserialize event_properties and update event dict variable_value
            if event['event_properties'] == "" or event['event_properties'] == None or event['event_properties'] == '""':
                continue
            for k,v in event['event_properties'].items():
                if k == self.algorithm_configurations['rec_item_identifier']: #set variable value as the rec_item_identifier
                    event_dict['variable_value']=v
            
            events_list.append(event_dict)

        self.events_df = pd.DataFrame(events_list, columns=Config['EVENTS_DATAFRAME_COLS'])
        self.logger.info('Built events dataframe')

    async def _get_items_data(self) -> list: 
        items_data = await recommendationsRepository.get_items(self.account_id)
        return items_data

    async def _build_items_dataset(self) -> None:
        self.logger.info('Building items dataset')
        items = await self._get_items_data()
        if len(items)< Config['MIN_NUM_ITEMS']:
            raise Exception('Not enough items found to conduct model training.')
            
        self.items_df=pd.DataFrame(items)

        #drop columns
        self.items_df=self.items_df.drop(['status', 'created_at', 'updated_at'], axis=1)
        
        #rename columns
        self.items_df.columns=Config['ITEMS_DATAFRAME_COLS']

        #insert required LFM_IDX into items_df. Sequential numerical IDS relative to each rows index. 
        self.items_df.insert(0, 'item_LFM_IDX', range(0, 0+len(self.items_df)))

    async def _required_gb(self, num_bytes) -> int:
        step_unit = 1000.0
        for unit in ['bytes', 'KB', 'MB', 'GB', 'TB']:
            if num_bytes < step_unit:
                self.logger.info(f"{int(num_bytes)}{unit}")
                if unit == 'GB':
                    return int(num_bytes+1) #Add 1 to ensure rounding doesn't create a memory discrepancy
                #elif prior to GB, return 1 GB
                elif unit == 'bytes' or unit == 'KB' or unit == 'MB':
                    return 1
                #elif after GB, multiply num by 1000, as there are 1000 GB in 1 TB
                elif unit == 'TB':
                    return int((num_bytes*1000)+1)

            num_bytes /= step_unit

    async def _chunk_file_(self, csv_object : dict, file_size : int) -> Union[bool, list, int]:
  
        file_data_=csv_object['data']
        #determine if csv_object data is of type string or byte
        if type(file_data_) is str:
            #convert to bytes, then to byte wrapper
            file_data_ = io.BytesIO(str.encode(file_data_))
        elif type(file_data_) is bytes:
            #convert to byte wrapper
            file_data_ = io.BytesIO(file_data_)


        chunk_count = math.ceil(file_size / int(Config['MIN_CHUNK_SIZE']))
        self.logger.info(f"Number of upload parts: {chunk_count}")

        if chunk_count > int(Config['MAX_NUM_PARTS']):
            raise Exception('Maximum number of chunk parts exceeded')

        start, i, chunks = 0, 0, []
        while i < chunk_count:

            end = min(file_size, start + int(Config['MIN_CHUNK_SIZE']))
            file_data_.seek(start)
            data = file_data_.read(end - start)
            start = end

            chunks.append(
                {
                    'data' : data,
                    'chunk_idx' : int(i+1)
                }
            )

            i+=1
        
        if len(chunks) < 2:
            return False, None, None
        return True, chunks, chunk_count

    async def _upload_resources(self) -> None:
        self.logger.info('Uploading training job resources...')
        for csv_object in self.csv_objects:
            #determine size of file
            file_size = sys.getsizeof(csv_object['data'])
            if file_size  < int(Config['MIN_FILE_SIZE']):
                self.key = await bucketRepository.single_upload(file_data=csv_object['data'],
                                                    resource_friendly_name=csv_object['type'],
                                                    training_job_id=self.training_job_id,
                                                    bucket_name=self.bucket)

            elif file_size  > int(Config['MIN_FILE_SIZE']) and file_size  < int(Config['MAX_FILE_SIZE']):

                res, chunks, chunk_count = await self._chunk_file_(csv_object, file_size)
                if not res:
                    raise Exception('Could not chunk file! Less thank 2 chunks.')
                
                for chunk in chunks:
                    self.logger.info(f'Uploading chunk: {chunk["chunk_idx"]} of: {chunk_count} ...')
                    is_last_chunk=False
                    if chunk['chunk_idx'] >= chunk_count:
                        is_last_chunk = True

                    if chunk['chunk_idx'] == 1:
                        #init MPU
                        self.key, self.mpu_upload_id, self.parts = \
                            await bucketRepository.multipart_upload(chunk_data=chunk['data'],
                                                                    chunk_index=chunk['chunk_idx'],
                                                                    resource_friendly_name=csv_object['type'],
                                                                    training_job_id=self.training_job_id,
                                                                    bucket_name=self.bucket)

                    else:
                        #upload_part
                        self.key, self.mpu_upload_id, self.parts = \
                            await bucketRepository.upload_part(chunk_data=chunk['data'],
                                                                chunk_index=chunk['chunk_idx'],
                                                                mpu_key=self.key,
                                                                upload_id=self.mpu_upload_id,
                                                                bucket_name=self.bucket,
                                                                parts=self.parts)
                        if is_last_chunk:
                            # complete MPU
                            await bucketRepository.complete_multipart_upload(mpu_key=self.key,
                                                                            upload_id=self.mpu_upload_id,
                                                                            bucket_name=self.bucket,
                                                                            parts=self.parts)
            else:
                raise Exception(f'Invalid file size. File size exceeds maximum. Account ID: {self.account_id} File type : {csv_object["type"]}')

            self.training_resources.append(
                {
                    'channel_name' : csv_object['type'],
                    'training_resource_location': self.key
                }
                )
            self.total_bytes += file_size
        self.logger.info('Uploaded training job resources!')

    async def _create_training_job_ref(self) -> None:
        meta_data = {'event_type' : self.algorithm_configurations['event_type'], 'features' : self.features}
        await recommendationsRepository.create_training_job_ref(items_df=self.items_df, 
                                                                profiles_df=self.profiles_df,
                                                                training_job_id=self.training_job_id,
                                                                account_id=self.account_id,
                                                                meta_data=meta_data)

    async def _start_cloud_training_job(self) -> None:
        self.logger.info('Starting cloud training...')
        #calculate required volume size
        volume_size = await self._required_gb(self.total_bytes)
        await recommendationsRepository.start_cloud_training(account_id=self.account_id, 
                                                            training_job_id=self.training_job_id, 
                                                            volume_size=volume_size, 
                                                            training_resources=self.training_resources,
                                                            bucket_name=self.bucket)
        self.logger.info('Cloud training started!')

    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                headers={'cursor': str(0)},
                timeout=5, 
                data=json.dumps(payload)
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            self.logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            self.logger.info(f'Took {t1 - t0}seconds')

    async def _dispose_job(self, ex : str) -> None:
        try:
            # Delete training job ref, if we have one
            await recommendationsRepository.delete_training_job_ref(account_id=self.account_id, 
                                                                    training_job_id=self.training_job_id)
            # abort_multipart_upload if self.mpu_upload_id != None
            await bucketRepository.abort_multipart_upload(key=self.key,
                                                        upload_id=self.mpu_upload_id, 
                                                        bucket_name=self.bucket)
            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : f'Recommender training Job failed. EX :: {ex}',
                'status' : 'failed'
            })
        except Exception as err:
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to dispose of job. {err}')

    async def _complete_job(self) -> None:
        self.logger.info('Successfully initated training job!')
        # HTTP call to confirm job completion with status
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
            'account_id' : self.account_id,
            'octy_job_id' : self.octy_job_id,
            'message' : 'Recommender training Job suceeded',
            'status' : 'success'
        })

        self.logger.info('Sending AMQP message to octy-job queue to create follow-up job!')
        # create follow up octy job to update training job status
        await amqpInterface.publish_message(routing_key='octy.job.cmd.create',
            message_payload={
                'account_id' : self.account_id,
                'job_type' : 'rec',
                'job_meta' : {
                    'desired_runs' : 1,
                    'time_interval' : 30,
                    'fail_threshold' : 3
                },
                'job_data' : {
                    'job_sub_type' : 'complete',
                    'training_job_id' : self.training_job_id
                }
        })

    async def run(self) -> None: 
        try:
            # Build training data sets
            await self._build_profiles_dataset()
            await self._build_items_dataset()
            await self._build_events_dataset()

            # feature engineering datasets
            await self._feature_engineering()

            # create csv datasets ready for upload
            await self._create_csv_objects()

            #upload training data resources 
            await self._upload_resources()

            # begin cloud training
            await self._start_cloud_training_job()

            # create DB ref
            await self._create_training_job_ref()

            await self._complete_job()

        except Exception as e:
            capture_exception(e)
            self.logger.critical(e)
            await self._dispose_job(ex=str(e))

class RecommenderCompleteTrainingJob():

    def __init__(self, account_id : str, algorithm_configurations : dict, octy_job_id : str, training_job_id : str, bucket : str, webhook_url : str):
        self.account_id = account_id
        self.algorithm_configurations = algorithm_configurations
        self.octy_job_id = octy_job_id
        self.training_job_id = training_job_id
        self.bucket = bucket
        self.webhook_url = webhook_url
        self.logger = logging.getLogger('uvicorn')
        self.status = 'InProgress'
        self.data_timeframe = Config['DATA_SET_TIMEFRAME']
        self.num_rec = 25 # number of recommendations per profile
        self.training_job = None
        self.lfm_idx_mappings = None
        self.predictions = list()
        self.model_meta = None
        self.model = None
        self.item_features = None
        self.all_items = list()
        self.seen_items = list()
        self.profiles_features = None
        self.profiles = None
        self.profiles_map = list()
        self.events = None

        item_stop_list = copy.deepcopy(self.algorithm_configurations['item_id_stop_list']) if self.algorithm_configurations['item_id_stop_list'] != None else []
        self.base_item_stop_list = []
        for i in item_stop_list:
            self.base_item_stop_list.append(i['item_id'])

    async def _get_cloud_training_status(self) -> str:
        self.logger.info('Getting cloud training status')
        status = await recommendationsRepository.get_cloud_training_status(training_job_id=self.training_job_id)
        return status
    
    async def _send_http_request(self, url : str, payload : dict) -> None:
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.post(
                url,
                timeout=5, 
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
                timeout=5, 
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

    async def _job_failed_webhook(self) -> None:
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has failed.',
            'body' : {
                'algorithm' : 'recommendations',
                'job_status' : 'Failed',
                'message' : 'An unknown server error occurred when attempting to train a model using this algorithm. You do have to do anything, we are aware of this issue and will resolve it shortly. Octy support team'
            },
            'date_time' : str(dt.now())
        })
    
    async def _job_success(self) -> None:
        await recommendationsRepository.update_training_job_ref(account_id=self.account_id,
                                                    training_job_id=self.training_job_id,
                                                    model_meta=self.model_meta,
                                                    status=self.status)
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has successfully completed.',
            'body' : {
                'algorithm' : 'recommendations',
                'job_status' : 'Completed',
                'message' : 'This means a new, up to date recommendations model is available for predictions. Octy support team.'
            },
            'date_time' : str(dt.now())
        })
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'Recommender training Job successfully completed',
                'status' : 'success'
        })

    async def _re_schedule_job(self) -> None:
        self.logger.info('Rescheduling job')
        # job is still processing
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'Recommender training Job still processing',
                'status' : 'failed' # send failed so next octy job tick can re-run it.
        })
        
    async def _destroy_job(self) -> None:
        try:
            self.logger.warning('Destroying job due to error')
            # delete training job artefacts 
            await bucketRepository.delete_directory(bucket_name=self.bucket, 
                                                    directory_path=f"{Config['REC_DATA_DIR']}/{self.training_job_id}")
            # Update training job reference to 'failed'
            await recommendationsRepository.update_training_job_ref(account_id=self.account_id,
                                                                training_job_id=self.training_job_id,
                                                                status='Failed')
            # Delete Octy job
            await amqpInterface.publish_message(routing_key='octy.job.cmd.delete',
                message_payload={
                    "account_id" : self.account_id,
                    "octy_job_ids" : [self.octy_job_id],
                    "alt_identifiers" : None
                })

            await self._job_failed_webhook()
        except Exception as err:
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to destroy job. {str(err)}')

    # Recommendations Predictions Cache methods

    async def _get_job_artifacts(self) -> None:
        self.logger.info('Getting job artifacts')

        self.training_job = await recommendationsRepository.get_training_job(account_id=self.account_id, \
            training_job_id=self.training_job_id, status='in_progress')
        
        model_location =  f"{Config['REC_MODELS_DIR']}/{self.training_job_id}/output/model.tar.gz"
        files = await bucketRepository.download_resource(bucket_name=self.bucket,
                                                        key=model_location,
                                                        is_compressed=True)
        for file_bytes in files:
            if file_bytes['file_name'] == 'model_meta_data.json':
                self.model_meta = json.loads(file_bytes['file_data'].read())
            elif file_bytes['file_name'] == 'trained_recommender_model.pkl':
                self.model = joblib.load(BytesIO(file_bytes['file_data'].read()))
            elif file_bytes['file_name'] == 'lfm_item_features.pkl':
                self.item_features = joblib.load(BytesIO(file_bytes['file_data'].read()))
            elif file_bytes['file_name'] == 'lfm_profile_features.pkl':
                self.profiles_features = joblib.load(BytesIO(file_bytes['file_data'].read()))

    async def _filter_lfm_idx_mappings(self, lfm_idx_mappings : dict, type_ : str = None, id_ : str = None) -> list:
        if id_!=None:
            return list(filter(lambda x : x['res_id'] == id_, lfm_idx_mappings))
        return list(filter(lambda x : x['type_'] == type_, lfm_idx_mappings))

    async def _get_lfm_idx_mappings(self) -> None: 
        self.lfm_idx_mappings = self.training_job['lfm_idxs']

    async def _get_items(self) -> None: 
        self.logger.info('Getting items')
        self.all_items = await recommendationsRepository.get_items(account_id=self.account_id, ids='true', status='active')
        if len(self.all_items) < 1:
            raise Exception('There are currently no active items associated with this account.')
        self.seen_items = await self._filter_lfm_idx_mappings(lfm_idx_mappings=self.lfm_idx_mappings, type_='items')

    async def _get_profiles(self) -> None: 
        self.logger.info('Getting profiles')
        self.profiles = await recommendationsRepository.get_profiles(account_id=self.account_id, ids='true')
        if len(self.profiles) < 1:
            raise Exception('There are currently no active profiles associated with this account.')  

    async def _get_profile_events(self) -> None: 
        self.logger.info('Getting charged events')
        self.events = await recommendationsRepository.get_events(account_id=self.account_id, 
                                                                profile_ids=self.profiles,
                                                                timeframe=self.data_timeframe,
                                                                event_type='charged')
        if len(self.events) < 1:
            raise Exception('There are currently no events associated with the provided profile ids.') 

    async def _filter_events(self, profile_id : str, events : list) -> None:
        return list(filter(lambda x : x['profile_id'] == profile_id, events))

    async def _build_profile_map(self) -> None:
        self.logger.info('Building profile map')
        # iterate over profiles and determine if profile exists in lfm idx mappings. 
        # Predictions can only be performed on 'seen' profiles
        for profile in self.profiles:
            profile_lfm_idx = await self._filter_lfm_idx_mappings(lfm_idx_mappings=self.lfm_idx_mappings, id_=profile)
            if len(profile_lfm_idx)>=1:
                self.profiles_map.append(
                    {
                        'profile_id' : profile,
                        'profile_LFM_IDX' : profile_lfm_idx[0]['lfm_idx']
                    }
                )
        # ensure there is at least one singular valid profile.
        if len(self.profiles_map) < 1:
            raise Exception('Error occurred when attempting to make item recommendations. No active profiles found.')

    async def _build_stop_list(self, profile_id : str) -> list:
        self.logger.info(f'Building stop list for profile: {profile_id}')
        profile_stop_list = copy.deepcopy(self.base_item_stop_list)
        if self.algorithm_configurations['recommend_interacted_items']:
            return profile_stop_list
        events = await self._filter_events(profile_id=profile_id, events=self.events)
        if len(events) <1:
            return profile_stop_list

        for event in events:
            #deserialize event_properties and update event dict variable_value
            item_id=None
            if event['event_properties'] == "" or event['event_properties'] == None or event['event_properties'] == '""':
                continue
            for k,v in event['event_properties'].items():
                if k == self.algorithm_configurations['rec_item_identifier']:
                    item_id=v
            if not item_id:
                continue
            profile_stop_list.append(
                item_id
            )
        return profile_stop_list

    async def _sort_filter_item_profile_scores(self, item_scores : list, profile_stop_list : list) -> list:
        sorted_scores = sorted(item_scores, key=lambda k: k['score'], reverse=True)
        filtered_scores = [x for x in sorted_scores if x['item_id'] not \
            in profile_stop_list and x['item_id'] in self.all_items][:self.num_rec]
        return filtered_scores

    async def _calculate_dot_products(self) -> None:
        #init required objects for dot product calculations 
        users_biases = self.model.get_user_representations()[0]
        users_embeddings = self.model.get_user_representations()[1]
        items_biases = self.model.get_item_representations()[0]
        items_embeddings = self.model.get_item_representations()[1]

        for profile in self.profiles_map:
            self.logger.info(f'Calculating Dot Product for profile: {profile["profile_id"]}')
            item_scores = []
            profile_stop_list = await self._build_stop_list(profile_id=profile['profile_id'])

            user = users_embeddings[profile['profile_LFM_IDX']]
            user_biases = users_biases[profile['profile_LFM_IDX']]

            # iterate over items & calculate dot product for each
            for item in self.seen_items:
                item_e = items_embeddings[item['lfm_idx']]
                item_biases = items_biases[item['lfm_idx']]
                result = user.dot(item_e.T)
                score = result + user_biases + item_biases

                item_scores.append(
                    {
                        'item_id' : item['res_id'],
                        'score' : float(str(score))
                    }
                )

            filtered_scores = await self._sort_filter_item_profile_scores(item_scores=item_scores, 
                                                                        profile_stop_list=profile_stop_list)
            if len(filtered_scores) < 1:
                continue
            
            self.predictions.append(
                {
                    'profile_id' : profile['profile_id'],
                    'item_scores' : filtered_scores
                }
            )

    async def _cache_item_recommendations(self) -> None:
        self.logger.info(f'Caching item recommendations')
        await recommendationsRepository.cache_item_recommendations(account_id=self.account_id,
                                                                training_job_id=self.training_job_id,
                                                                predictions=self.predictions)

    async def _make_predictions(self) -> None:
        await self._get_job_artifacts()
        await self._get_lfm_idx_mappings()
        await self._get_items()
        await self._get_profiles()
        await self._build_profile_map()
        if not self.algorithm_configurations['recommend_interacted_items']:
            await self._get_profile_events()
        await self._calculate_dot_products()
        await self._cache_item_recommendations()
        await self._job_success()


    async def run(self) :

        try:
            #Switch through job status
            self.status = await self._get_cloud_training_status()

            self.logger.info(f'Job ID : {self.training_job_id} -- Status : {self.status}')

            if self.status == 'InProgress':
                await self._re_schedule_job()
            
            elif self.status == 'Completed':
                await self._make_predictions()

            elif self.status == 'Failed':
                await self._destroy_job()

            elif self.status == 'Stopping':
                await self._destroy_job()

            elif self.status == 'Stopped':
                await self._destroy_job()

        except Exception as e:
            self.logger.critical(str(e))
            capture_exception(e)
            await self._re_schedule_job()