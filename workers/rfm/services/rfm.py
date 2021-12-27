# module imports
from data.repositories.implementation.rfm_repository import rfmRepository
from data.repositories.implementation.bucket_repository import bucketRepository
from .billing import BillingUnits
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
import sys


# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from sentry_sdk import capture_exception
import pandas as pd
import joblib
import numpy as np
from sklearn.preprocessing import LabelEncoder
from sklearn.cluster import KMeans
from kneed import KneeLocator


class RFMAnalysis():

    def __init__(self, account_id : str, account_type : str, account_currency : str, octy_job_id : str, bucket : str, loop : Any):
        self.account_id = account_id
        self.octy_job_id = octy_job_id
        self.bucket = bucket
        self.loop = loop
        self.b = BillingUnits(account_id=account_id, account_type=account_type, account_currency=account_currency, process_name='rfm_analysis', loop=loop)
        self.logger = logging.getLogger('uvicorn.error')
        self.training_job_id = generate_uid('training-job')
        self.data_timeframe = Config['DATA_SET_TIMEFRAME']
        self.training_df = None
        self.profiles_ids = None
        self.items_data = None
        self.events_data = None
        #self.event_item_map = []
        self.csv_objects = []
        self.training_resources = []
        self.total_bytes=0
        self.mpu_upload_id = None
        self.key = None
        self.parts = None


    async def _dataframe_to_csv_obj(self, dataframe : object, type_ : str) -> dict:
        return  {
            'data' : dataframe.to_csv(index=False),
            'type' : type_
        }

    async def _create_csv_objects(self) -> None:
        # convert df to csv and store in csv_objects array
        training_csv = await self._dataframe_to_csv_obj(self.training_df, 'training')
        self.csv_objects.extend([training_csv])

    async def _get_profile_ids(self) -> None: 
        self.profiles_ids = await rfmRepository.get_profiles(self.account_id, ids='true')

    async def _get_items_data(self) -> None: 
        self.items_data = await rfmRepository.get_items(self.account_id)

    async def _get_events_data(self) -> None:
        self.events_data = await rfmRepository.get_events(self.account_id, 
                                                        self.profiles_ids, 
                                                        self.data_timeframe, 
                                                        'charged')
    
    async def _filter_events(self, events : list, profile_id : str) -> list:
        return list(filter(lambda x : x['profile_id'] == profile_id, events))
    
    async def _filter_items(self, items : list, item_ids : str) -> list:
        return list(filter(lambda x : x['item_id'] in item_ids, items))

    def _build_event_item_object(self, event : dict) -> dict:
        event_dict ={
            'profile_id' : event['profile_id'],
            'item_price': 0,
            'created_at' : str_to_dt(event['created_at'])
        }

        if event['event_properties'] == "" or event['event_properties'] == None or event['event_properties'] == '""':
            return event_dict
        
        for k,v in event['event_properties'].items():
            if k == 'item_id':
                item =  next((i for i in self.items_data if i["item_id"] == v), None)
                if item:
                    item_price = item['item_price']
                    event_dict['item_price'] = item_price
        return event_dict

    #**
    async def _build_training_df(self) -> None:
        self.logger.info('Building RFM analysis dataset...')
        await self._get_items_data()
        if len(self.items_data)< Config['MIN_NUM_ITEMS']:
            raise Exception('Not enough items found.')
        await self._get_profile_ids()
        if len(self.profiles_ids) < Config['MIN_NUM_PROFILES']:
            raise Exception('Not enough active profiles found.')
        await self._get_events_data()
        if len(self.events_data)< Config['MIN_NUM_EVENTS_COLLECTIVE']:
            raise Exception('Not enough events found to conduct rfm analysis.')
        
        #await self._build_event_item_map()
        self.event_item_map = (self._build_event_item_object(event) for event in self.events_data) # generator

        self.training_df = pd.DataFrame(self.event_item_map)
        zero_indicies = self.training_df[ self.training_df['item_price'] == 0 ].index
        self.training_df.drop(zero_indicies, inplace = True)
        self.logger.info('Created RFM analysis dataset!')


    async def _validate_recency_unique(self) -> None:
        #validate:: unique recency days > 10
        tx_max_purchase = self.training_df.groupby('profile_id').created_at.max().reset_index()
        tx_max_purchase.columns = ['profile_id','max_created_at']
        tx_max_purchase['recency'] = (tx_max_purchase['max_created_at'].max() - tx_max_purchase['max_created_at']).dt.days
        if len(tx_max_purchase['recency'].unique()) < 10:
            raise Exception('Number of required unique recency days not met. >10 required.')

    async def _validate_frequency_unique(self) -> None:
        #validate :: unique number of frequencies
        tx_frequency = self.training_df.groupby('profile_id').created_at.count().reset_index()
        tx_frequency.columns = ['profile_id','frequency']
        if len(tx_frequency['frequency'].unique()) < 10:
            raise Exception('Number of required unique frequent events not met. >10 required.')

    async def _validate_monetary_unique(self) -> None:
        #validate :: unique number of monetary values
        tx_revenue = self.training_df.groupby('profile_id').item_price.sum().reset_index()
        if len(tx_revenue['item_price'].unique()) < 10:
            raise Exception('Number of required unique monetary amounts not met. >10 required.')

    #**
    async def _training_df_validation(self) -> None:
        await self._validate_recency_unique()
        await self._validate_frequency_unique()
        await self._validate_monetary_unique()


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

    #**
    async def _upload_resources(self) -> None:
        self.logger.info('Uploading training job resources')
        for csv_object in self.csv_objects:
            self.b.track_data_units(csv_object)
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

        self.b.complete_data_units('MB')
        self.logger.info('Uploaded training job resources!')
    #**
    async def _create_training_job_ref(self) -> None:
        await rfmRepository.create_training_job_ref(training_job_id=self.training_job_id,
                                                    account_id=self.account_id)

    #**
    async def _start_cloud_training_job(self) -> None:
        self.logger.info('Starting cloud training')
        #calculate required volume size
        volume_size = await self._required_gb(self.total_bytes)
        await rfmRepository.start_cloud_training(account_id=self.account_id, 
                                                training_job_id=self.training_job_id, 
                                                volume_size=volume_size, 
                                                training_resources=self.training_resources,
                                                bucket_name=self.bucket)


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
    #**
    async def _dispose_job(self, ex : str) -> None:
        try:
            # Delete training job ref, if we have one
            await rfmRepository.delete_training_job_ref(account_id=self.account_id, 
                                                        training_job_id=self.training_job_id)
            # abort_multipart_upload if self.mpu_upload_id != None
            await bucketRepository.abort_multipart_upload(key=self.key,
                                                        upload_id=self.mpu_upload_id, 
                                                        bucket_name=self.bucket)
            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : f'RFM analysis Job failed. EX :: {ex}',
                'status' : 'failed'
            })
        except Exception as err:
            self.b.complete_compute_units()
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to dispose of job. {str(err)}')

    #**
    async def _complete_job(self) -> None:
        self.logger.info('Training job compelte')
        # HTTP call to confirm job completion with status
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
            'account_id' : self.account_id,
            'octy_job_id' : self.octy_job_id,
            'message' : 'RFM analysis Job suceeded',
            'status' : 'success'
        })

        # create follow up octy job to update training job status
        self.loop.create_task(amqpPublisher.send_message(routing_key='octy.job.cmd.create',
            payload={
                'account_id' : self.account_id,
                'job_meta' : {
                    'job_type' : 'rfm',
                    'amqp_routing_key': 'rfm.training.complete.cmd.run',
                    'required_permissions' : ['rfm'],
                    'required_configurations' :
                        { 
                            'account_attributes' : [
                                'account_configurations.webhook_url',
                                'bucket'
                            ],
                            'algorithm_configuration_idxs' : [
                            ]
                        },
                    'desired_runs' : 1,
                    'time_interval' : 30,
                    'fail_threshold' : 3
                },
                'job_data' : {
                    'training_job_id' : self.training_job_id
                }
        }))


    async def run(self) -> None: 
        try:
            self.b.track_compute_units('hours')
            # Build training data sets
            await self._build_training_df()
            await self._training_df_validation()

            # create csv datasets ready for upload
            await self._create_csv_objects()

            #upload training data resources 
            await self._upload_resources()

            # begin cloud training
            await self._start_cloud_training_job()

            # create DB ref
            await self._create_training_job_ref()

            await self._complete_job()

            self.b.complete_compute_units()
            self.logger.info('Completed Job!')

        except Exception as e:
            capture_exception(e)
            self.logger.critical(str(e))
            self.b.complete_compute_units()
            await self._dispose_job(ex=str(e))


class RFMCompleteAnalysis():

    def __init__(self, account_id : str, account_type : str, account_currency : str, octy_job_id : str, bucket : str, training_job_id : str, webhook_url : str, loop : Any):
        self.account_id = account_id
        self.octy_job_id = octy_job_id
        self.bucket = bucket
        self.training_job_id = training_job_id
        self.webhook_url = webhook_url
        self.loop = loop
        self.b = BillingUnits(account_id=account_id, account_type=account_type, account_currency=account_currency, process_name='rfm_analysis_completion', loop=loop)
        self.logger = logging.getLogger('uvicorn.error')
        self.rfm_scores_df = None
        self.amqp_message_size_limit = 104857600 #100 MB AMQP message limit
        self.training_compute_units = 0

    async def _get_cloud_training_status(self) -> str:
        self.logger.info('Getting cloud training status')
        status, self.training_compute_units = await rfmRepository.get_cloud_training_status_time(training_job_id=self.training_job_id)
        return status
    
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

    async def _job_failed_webhook(self) -> None:
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has failed.',
            'body' : {
                'algorithm' : 'rfm-analysis',
                'job_status' : 'Failed',
                'message' : 'An unknown server error occurred when attempting to conduct RFM analysis. You do have to do anything, we are aware of this issue and will resolve it shortly. Octy support team'
            },
            'date_time' : str(dt.now())
        })
    
    async def _job_success(self) -> None:
        await rfmRepository.update_training_job_ref(account_id=self.account_id,
                                                    training_job_id=self.training_job_id,
                                                    status=self.status)
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has successfully completed.',
            'body' : {
                'algorithm' : 'rfm-analysis',
                'job_status' : 'Completed',
                'message' : 'This means a new, up to date RFM score has been applied to each profile. Octy support team.'
            },
            'date_time' : str(dt.now())
        })
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'RFM analysis Job successfully completed',
                'status' : 'success'
        })

    async def _re_schedule_job(self) -> None:
        self.logger.info('Rescheduling job')
        # job is still processing
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'RFM analysis Job still processing',
                'status' : 'failed' # send failed so next octy job tick can re-run it.
        })
        
    async def _destroy_job(self) -> None:
        try:
            self.logger.warning('Destroying job due to error')
            # delete training job artefacts 
            await bucketRepository.delete_directory(bucket_name=self.bucket, 
                                                    directory_path=f"{Config['RFM_MODELS_DIR']}/{self.training_job_id}")
            # Update training job reference to 'failed'
            await rfmRepository.update_training_job_ref(account_id=self.account_id,
                                                                training_job_id=self.training_job_id,
                                                                status='Failed')
            # Delete Octy job
            self.loop.create_task(amqpPublisher.send_message(routing_key='octy.job.cmd.delete',
                payload={
                    "account_id" : self.account_id,
                    "octy_job_ids" : [self.octy_job_id],
                    "alt_identifiers" : None
                }))

            await self._job_failed_webhook()
        except Exception as err:
            self.b.complete_compute_units()
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to destroy job. {str(err)}')


    async def _get_job_artifacts(self) -> None:
        self.logger.info('Getting job artifacts')

        self.training_job = await rfmRepository.get_training_job(account_id=self.account_id, \
            training_job_id=self.training_job_id, status='in_progress')
        
        model_location =  f"{Config['RFM_MODELS_DIR']}/{self.training_job_id}/output/model.tar.gz"
        files = await bucketRepository.download_resource(bucket_name=self.bucket,
                                                        key=model_location,
                                                        is_compressed=True)
        for file_bytes in files:
            if file_bytes['file_name'] == 'df_scores.pkl':
                self.rfm_scores_df = joblib.load(BytesIO(file_bytes['file_data'].read()))

    async def _assign_rfm_scores(self) -> None:
        amqp_batch_profiles = []
        # convert predictions_df to list of dictonaries
        predictions_dicts = self.rfm_scores_df.to_dict('records')
        profile_updates = []
        for pd in predictions_dicts:

            profile_updates.append(
                {
                    'profile_id' : pd['profile_id'],
                    'rfm_score' : pd['rfm_score'],
                    'rfm_segment_desc' : pd['segment_description']
                }
            )

            # split profile objects according to AMQP message size limit.
            if sys.getsizeof(profile_updates) > self.amqp_message_size_limit:
                amqp_batch_profiles.append(profile_updates)
                # flush profile_updates
                profile_updates = []
        
        # message limit no exceeded, append all profile_updates
        if len(amqp_batch_profiles) < 1:
            amqp_batch_profiles.append(profile_updates)

        for profiles_updates in amqp_batch_profiles:
            self.loop.create_task(amqpPublisher.send_message(routing_key='profiles.cmd.update',
                payload={
                    'account_id' : self.account_id,
                    'profiles' : profiles_updates  
                }))


    async def run(self) :

        try:
            #Switch through job status
            self.status = await self._get_cloud_training_status()

            self.logger.info(f'Job ID : {self.training_job_id} -- Status : {self.status}')

            if self.status == 'InProgress':
                await self._re_schedule_job()
            
            elif self.status == 'Completed':
                self.b.track_compute_units('hours')
                await self._get_job_artifacts()
                await self._assign_rfm_scores()
                await self._job_success()
                self.b.complete_compute_units(additional_unit_hours=self.training_compute_units)

            elif self.status == 'Failed':
                await self._destroy_job()

            elif self.status == 'Stopping':
                await self._destroy_job()

            elif self.status == 'Stopped':
                await self._destroy_job()

        except Exception as e:
            self.logger.critical(str(e))
            capture_exception(e)
            self.b.complete_compute_units(additional_unit_hours=self.training_compute_units)
            await self._re_schedule_job()
