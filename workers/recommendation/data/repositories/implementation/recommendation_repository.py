# module imports
from data.repositories.Irecommendation_repository import RecommendationsInterface
from utils.utils import *
from data.models.db_schemas import *
from config import Config
from secrets import Secrets

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import time
import logging

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps
import boto3
from botocore.exceptions import ClientError
from sentry_sdk import capture_exception


class _RecommendationsRepository(RecommendationsInterface):
    """
        _RecommendationsRepository
        Handles:
        - Getting raw training data
        - Create training recommendations job ref

        ...

        Attributes
        ----------
        none
    """
    def __init__(self):
        # Initialize s3 client object
        self.s3_client = boto3.client('sagemaker',
         region_name=Config['AWS_REGION'],
         aws_access_key_id=Secrets['AWS_ACCESS_KEY_ID'],
         aws_secret_access_key= Secrets['AWS_SECRET_ACCESS_KEY'])

    async def get_events(self, account_id : str, profile_ids : list, timeframe : int, event_type : str) -> list:
        """
        Parameters
        ----------
        account_id : str
        profile_ids : list
        timeframe : int
        event_type : str

        Returns
        ----------
        events : list
        """
        logger = logging.getLogger('uvicorn')
        url = f"{Config['EVENT_SERVICE_CLUSTER_IP']}/v1/internal/events"
        events = []
        exhausted_events = False
        cursor : int = 0
        session = requests_retry_session()

        payload = {
            'timeframe' : timeframe,
            'account_id' : account_id,
            'profile_ids' : profile_ids,
            'event_type' : event_type
        }

        while not exhausted_events:
            t0 = time.time()
            try:
                #logger.info(f"Pre request Cursor: {cursor}")
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=5
                )
            except Exception as x:
                raise Exception(x) from None
            else:
                logger.info(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
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
            #logger.info(f"Returned {body['request_meta']['count']} events with this request. Total: {body['request_meta']['total']}")
            cursor += body['request_meta']['count']
            # logger.info(f"Next Cursor {cursor}")
            # logger.info("================================================")

        #logger.info(f"Returning found events data from repo")
        return events
    
    async def get_profiles(self, account_id : str, ids : str = 'false') -> list:
        """
        Parameters
        ----------
        account_id : str
        ids : str

        Returns
        ----------
        profiles : list
        """
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids={ids}"
        profiles = []
        payload = {
            'account_id': account_id,
            'profiles' : [], 
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
                if ids == 'true':
                    profiles.append(
                        profile['profile_id']
                    )
                else:
                    profiles.append(
                        profile
                    )
            cursor +=body['request_meta']['count']

        return profiles

    async def get_items(self, account_id : str, ids : str = 'false', status : str = 'all') -> list:
        """
        Parameters
        ----------
        account_id : str
        ids : str
        status : str

        Returns
        ----------
        items : list
        """
        url = f"{Config['ITEM_SERVICE_CLUSTER_IP']}/v1/internal/items?account_id={account_id}&ids={ids}&status={status}"
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
                exhausted_items = True
                continue

            body = json.loads(response.text)
            for item in body['items']:
                if ids == 'true':
                    items.append(
                        item['item_id']
                    )
                else:
                    items.append(
                        item
                    )
            cursor +=body['request_meta']['count']

        return items

    async def get_segments(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        segments : list
        """
        url = f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments?account_id={account_id}&segment_type=all&status=active"
        segments = []
        session = requests_retry_session()
        t0 = time.time()
        try:
            response = session.get(
                url,
                timeout=5
            )
        except Exception as x:
            raise Exception(x) from None
        else:
            print(f'{response.request.method} Request: "{url}" returned response with valid status code: {response.status_code}')
        finally:
            t1 = time.time()
            print('Took', t1 - t0, 'seconds')

        body = json.loads(response.text)
        for segment in body['segments']:
            segments.append(
                segment
            )

        return segments

    async def create_training_job_ref(self, items_df : object, profiles_df : object, training_job_id : str, account_id : str, meta_data : dict) -> None:
        """
        Parameters
        ----------
        items_df : object
        profiles_df : object
        training_job_id : str
        account_id : str
        meta_data : dict

        Returns
        ----------
        None
        """
        lfm_idx_map=[]
        # convert both dataframes to lists and iterate to build batch insert db object. 
        # the index of each row can be used as the LFM_IDX as each row index should represent this value
        async def build_input(_id_list, type_):
            for i, id_ in enumerate(_id_list):
                lfm_idx_map.append(lfm(
                    lfm_idx=i,
                    type_=type_,
                    res_id=id_
                ))

        await build_input(profiles_df['profile_id'].to_list(), 'profiles')
        await build_input(items_df['item_id'].to_list(), 'items')

        job_ref = tbl_training_jobs(
            training_job_id=training_job_id,
            account_id=account_id, 
            meta_data=meta_data,
            lfm_idxs=lfm_idx_map
        )
        job_ref.save()

    async def get_training_job(self, account_id : str, training_job_id : str, status : str) -> dict:
        """
        Parameters
        ----------
        training_job_id : str
        account_id : str
        status : str

        Returns
        ----------
        training job : dict
        """
        return json.loads(tbl_training_jobs.objects\
            .get(account_id__exact=account_id, training_job_id__exact=training_job_id, status__exact=status).to_json())
    
    async def delete_training_job_ref(self, account_id : str, training_job_id : str) -> None:
        """
        Parameters
        ----------
        training_job_id : str
        account_id : str

        Returns
        ----------
        None
        """
        tbl_training_jobs.objects(account_id__exact=account_id,training_job_id__exact=training_job_id).delete()

    async def start_cloud_training(self, account_id : str, 
                                training_job_id : str, 
                                volume_size : int, 
                                training_resources : list, 
                                bucket_name : str) -> None:
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        volume_size : int
            required volume storage for training job.
        training_resources : list
        bucket_name : str

        Returns
        ----------
        None
        """
        input_mode = Config['RECOMMENDATION_SM_INPUT_MODE']
        out_path = Config['REC_MODELS_DIR']
        training_image = Config['RECOMMENDATION_ALGORITHM_DOCKER_PATH']
        hyper_parameters = Config['REC_TRAINING_HYPERPARAMETERS']

        input_data=[]
        for training_res in training_resources:
            input_data.append(
                {
                    'ChannelName': training_res['channel_name'],
                    'DataSource': {
                        'S3DataSource': {
                            'S3DataType': 'S3Prefix',
                            'S3Uri': f's3://{bucket_name}/{training_res["training_resource_location"]}',
                            'S3DataDistributionType': 'FullyReplicated'
                        }
                    },
                    'ContentType': 'text/csv', #text/csv | application/x-recordio-protobuf
                    'CompressionType': 'None',
                    'RecordWrapperType': 'None',
                    'InputMode': input_mode
                }
            )
        
        self.s3_client.create_training_job(
            TrainingJobName=training_job_id,
            HyperParameters=hyper_parameters,
            AlgorithmSpecification={
                'TrainingImage': training_image,
                'TrainingInputMode': input_mode
            },
            RoleArn=Config['AWS_ROLE_ARN'],
            InputDataConfig=input_data,
            OutputDataConfig={
                'S3OutputPath': f's3://{bucket_name}/{out_path}'
            },
            ResourceConfig={
                'InstanceType': Config['EC2_INSTANCE_TYPE'],
                'InstanceCount': 1,
                'VolumeSizeInGB': volume_size
            },
            StoppingCondition={
                'MaxRuntimeInSeconds': Config['TRAINING_MAX_RUN_TIME']
            },
            Tags=[
                {
                    'Key': 'octy_account_id',
                    'Value': account_id
                }
            ]
        )

    async def get_cloud_training_status(self, training_job_id : str) -> str:
        """
        Parameters
        ----------
        training_job_id : str

        Returns
        ----------
        status : str
        """
        return self.s3_client.describe_training_job(TrainingJobName=training_job_id)['TrainingJobStatus']

    async def update_training_job_ref(self, account_id : str, training_job_id : str, status : str, model_meta : dict = None) -> None:
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        status : str

        Returns
        ----------
        None
        """
        if model_meta:
            tbl_training_jobs.objects(account_id__exact=account_id, \
                training_job_id__exact=training_job_id).update(set__status=status, set__model_meta_data=model_meta, set__updated_at=dt.now())
            return 

        tbl_training_jobs.objects(account_id__exact=account_id, \
            training_job_id__exact=training_job_id).update(set__status=status, set__updated_at=dt.now())

    async def cache_item_recommendations(self, account_id : str, training_job_id : str, predictions : list) -> None:
        """
        Parameters
        ----------
        account_id : str
        training_job_id : str
        predictions : list

        Returns
        ----------
        None
        """
        predictions_mongo = list()
        for prediction in predictions:
            scores_mongo = list()
            for score in prediction['item_scores']:
                scores_mongo.append(
                    Recommendations(
                        score=score['score'],
                        item_id=score['item_id']
                    )
                )

            predictions_mongo.append(
                tbl_recommendations_cache(
                    account_id = account_id,
                    training_job_id = training_job_id,
                    profile_id=prediction['profile_id'],
                    recommendations=scores_mongo
                )
            )

        bulk_operation = tbl_recommendations_cache._get_collection().initialize_unordered_bulk_op()
        for p_mongo in predictions_mongo:
            bulk_operation.insert(p_mongo.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            capture_exception(bwe)
            raise Exception('Error occurred when attempting to cache recommendations')

recommendationsRepository = _RecommendationsRepository()
