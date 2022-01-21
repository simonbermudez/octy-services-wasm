# module imports
from data.repositories.Ichurn_repository import ChurnPredInterface
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

# external imports
from mongoengine.errors import BulkWriteError, DoesNotExist
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps
import boto3
from botocore.exceptions import ClientError
from sentry_sdk import capture_exception


class _ChurnPredictionRepository(ChurnPredInterface):
    """
        _ChurnPredictionRepository
        Handles:
        - Getting raw training data
        - Crud operations on hyper-parameter tuning job references
        - Handling cloud based hyper-parameter tuning and training jobs

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
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=60
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
    
    async def get_profiles(self, account_id : str, status : str = 'active', ids : str = 'false') -> list:
        """
        Parameters
        ----------
        account_id : str
        status : str
        ids : str

        Returns
        ----------
        profiles : list
        """
        url = f"{Config['PROFILE_SERVICE_CLUSTER_IP']}/v1/internal/profiles?ids={ids}&status={status}"
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
                    timeout=60
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

    async def get_items(self, account_id : str, ids : str = 'false') -> list:
        """
        Parameters
        ----------
        account_id : str
        ids : str

        Returns
        ----------
        items : list
        """
        url = f"{Config['ITEM_SERVICE_CLUSTER_IP']}/v1/internal/items?account_id={account_id}&ids={ids}&status=all"
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
                    timeout=60
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

    async def get_segments(self, account_id : str, status : str = 'active') -> list:
        """
        Parameters
        ----------
        account_id : str
        status : str

        Returns
        ----------
        segments : list
        """
        url = f"{Config['SEGMENTATION_SERVICE_CLUSTER_IP']}/v1/internal/segments?account_id={account_id}&segment_type=all&status={status}"
        segments = []
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

        body = json.loads(response.text)
        if response.status_code < 202:
            for segment in body['segments']:
                segments.append(
                    segment
                )

        return segments

    async def create_hparam_tuning_job_ref(self, hyperparam_tuning_job_id : str, account_id : str, meta_data : dict) -> None:
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str
        account_id : str
        meta_data : dict

        Returns
        ----------
        None
        """
        job_ref = tbl_hparam_tuning_jobs(
            hyperparam_tuning_job_id=hyperparam_tuning_job_id,
            account_id=account_id, 
            meta_data=meta_data
        )
        job_ref.save()

    async def get_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str, status : str) -> dict:
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str
        account_id : str
        status : str

        Returns
        ----------
        tuning job reference : dict
        """
        return json.loads(tbl_hparam_tuning_jobs.objects\
            .get(account_id__exact=account_id, hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id, status__exact=status).to_json())
    
    async def get_parent_hparam_tuning_job_ref(self, account_id : str) -> dict:
        """
        Parameters
        ----------
        account_id : str


        Returns
        ----------
        latest 'Completed' hyper parameter tuning job : dict | None
        """
        try:
            parent_job = tbl_hparam_tuning_jobs.objects(account_id__exact=account_id, status__exact='Completed').order_by('-updated_at').first()
            return json.loads(parent_job.to_json())
        except:
            return None

    async def update_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str, best_model_training_job_id :str, status : str, model_meta : dict) -> None:
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str
        best_model_training_job_id :str
        status : str
        model_meta : dict

        Returns
        ----------
        None
        """
        if model_meta:
            tbl_hparam_tuning_jobs.objects(account_id__exact=account_id, \
                hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id)\
                    .update(set__status=status, 
                            set__best_model_meta_data=model_meta, 
                            set__best_model_training_job_id=best_model_training_job_id, 
                            set__updated_at=dt.now()
            )
            return 

        tbl_hparam_tuning_jobs.objects(account_id__exact=account_id, \
            hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id).update(set__status=status, set__updated_at=dt.now())

    async def delete_hparam_tuning_job_ref(self, account_id : str, hyperparam_tuning_job_id : str) -> None:
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str
        account_id : str

        Returns
        ----------
        None
        """
        tbl_hparam_tuning_jobs.objects(account_id__exact=account_id,hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id).delete()

    async def start_hparam_tuning_job(self, 
                            account_id : str, 
                            hyperparam_tuning_job_id : str,
                            parent_hyperparam_tuning_job_id : str or None,
                            volume_size : int, 
                            training_resources : list, 
                            bucket_name : str) -> None:
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str
        parent_hyperparam_tuning_job_id : str | None
        volume_size : int
            required volume storage for training job.
        training_resources : list
        bucket_name : str

        Returns
        ----------
        None
        """

        tuning_objective_metric = Config['CHURN_OBJECTIVE_METRIC']
        input_mode = Config['CHURN_SM_INPUT_MODE']
        out_path = Config['CHURN_PRED_MODELS_DIR']
        training_image = Config['CHURN_ALGORITHM_DOCKER_PATH']
        role_arn = Config['AWS_ROLE_ARN']
        max_runtime = Config['CHURN_TRAINING_MAX_RUN_TIME']
        churn_instance_type = Config['EC2_INSTANCE_TYPE']

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

        HyperParameterTuningJobConfig = {
                'Strategy': 'Bayesian',
                'HyperParameterTuningJobObjective': {
                    'Type': 'Maximize',
                    'MetricName': tuning_objective_metric
                },
                'ResourceLimits': {
                    'MaxNumberOfTrainingJobs': 1,
                    'MaxParallelTrainingJobs': 1
                },
                'ParameterRanges': {
                    'IntegerParameterRanges': [
                        {
                            'Name': 'n_estimators',
                            'MinValue': '139',
                            'MaxValue': '140'
                        },
                        {
                            'Name': 'min_child_weight',
                            'MinValue': '1',
                            'MaxValue': '2'
                        },
                        {
                            'Name': 'gamma',
                            'MinValue': '0',
                            'MaxValue': '1'
                        },
                        {
                            'Name': 'seed',
                            'MinValue': '26',
                            'MaxValue': '28'
                        },
                    ],
                    'ContinuousParameterRanges': [
                        {
                            'Name': 'learning_rate',
                            'MinValue': '0.1',
                            'MaxValue': '0.2'
                        },
                        {
                            'Name': 'colsample_bytree',
                            'MinValue': '0.7',
                            'MaxValue': '0.9'
                        },
                    ],

                },
                'TrainingJobEarlyStoppingType': 'Auto',
                'TuningJobCompletionCriteria': {
                    'TargetObjectiveMetricValue': 0.95
                }
            }

        TrainingJobDefinition={
            'StaticHyperParameters': {
                'nthread': Config['CHURN_TRAINING_STATIC_HYPERPARAMETERS']['nthread'],
                'max_depth': Config['CHURN_TRAINING_STATIC_HYPERPARAMETERS']['max_depth'],
                'subsample': Config['CHURN_TRAINING_STATIC_HYPERPARAMETERS']['subsample'],
                'objective': Config['CHURN_TRAINING_STATIC_HYPERPARAMETERS']['objective'],
                'scale_pos_weight': Config['CHURN_TRAINING_STATIC_HYPERPARAMETERS']['scale_pos_weight'],
            },
            'AlgorithmSpecification': {
                'TrainingImage': training_image,
                'TrainingInputMode': input_mode,
                'MetricDefinitions': [
                    {
                        'Name': tuning_objective_metric,
                        'Regex': tuning_objective_metric +'=(.*?);',
                    },
                ]
            },
            'RoleArn': role_arn,
            'InputDataConfig': input_data,
            'OutputDataConfig': {
                'S3OutputPath': f's3://{bucket_name}/{out_path}'
            },
            'ResourceConfig': {
                'InstanceType': churn_instance_type,
                'InstanceCount': 1,
                'VolumeSizeInGB': volume_size
            },
            'StoppingCondition': {
                'MaxRuntimeInSeconds': max_runtime
            }
        }

        Tags=[{
                'Key': 'octy_account_id',
                'Value': account_id
            },
        ]

        if parent_hyperparam_tuning_job_id:
            self.s3_client.create_hyper_parameter_tuning_job(
                HyperParameterTuningJobName=hyperparam_tuning_job_id,
                HyperParameterTuningJobConfig=HyperParameterTuningJobConfig,
                TrainingJobDefinition=TrainingJobDefinition,
                WarmStartConfig={
                'ParentHyperParameterTuningJobs': [
                    {
                        'HyperParameterTuningJobName': parent_hyperparam_tuning_job_id
                    },
                ],
                'WarmStartType': 'IdenticalDataAndAlgorithm'
            },
            Tags=Tags
            )
        else:
            self.s3_client.create_hyper_parameter_tuning_job(
                    HyperParameterTuningJobName=hyperparam_tuning_job_id,
                    HyperParameterTuningJobConfig=HyperParameterTuningJobConfig,
                    TrainingJobDefinition=TrainingJobDefinition,
                    Tags=Tags
                )

    async def get_hparam_tuning_job_status(self, hyperparam_tuning_job_id : str) -> str:
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str

        Returns
        ----------
        status : str
        """
        # Check status of both the tuning job and best training job.
        hpt_job = self.s3_client.describe_hyper_parameter_tuning_job(HyperParameterTuningJobName=hyperparam_tuning_job_id)
        if hpt_job['HyperParameterTuningJobStatus'] == 'InProgress':
            return hpt_job['HyperParameterTuningJobStatus']

        return hpt_job['BestTrainingJob']['TrainingJobStatus']

    async def get_best_training_job(self, hyperparam_tuning_job_id : str) -> Union[dict, int]:
        """
        Parameters
        ----------
        hyperparam_tuning_job_id : str

        Returns
        ----------
        best_training_job : dict
        training_compute_units (hyper parameter tuning job total hours) : int
        """
        hp_job = self.s3_client.describe_hyper_parameter_tuning_job(HyperParameterTuningJobName=hyperparam_tuning_job_id)
        diff =  hp_job['HyperParameterTuningEndTime'] - hp_job['CreationTime']
        training_compute_units = diff.total_seconds() / 3600
        job = hp_job['BestTrainingJob']
        return {
            'training_job_name': job['TrainingJobName'],
            'training_job_arn': job['TrainingJobArn'],
            'creation_time': job['CreationTime'],
            'training_start_time': job['TrainingStartTime'],
            'training_end_time': job['TrainingEndTime'],
            'training_job_status': job['TrainingJobStatus'],
            'tuned_hyper_parameters': job['TunedHyperParameters'],
            'final_hyper_parameter_tuning_job_objective_metric': job['FinalHyperParameterTuningJobObjectiveMetric'],
            'objective_status': job['ObjectiveStatus']
        }, training_compute_units if training_compute_units >= 1 else 1

    async def cache_dataset(self, account_id : str, hyperparam_tuning_job_id : str, dataset : object) -> None:
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str
        dataset : str

        Returns
        ----------
        None
        """
        dataset_rows = []
        for k, _ in dataset.items():
            # We only need to store profiles that have not yet churned
            if dataset[k]['churn'] == True:
                continue
            
            dataset_rows.append(
                tbl_training_dataset_cache(
                    account_id=account_id,
                    hyperparam_tuning_job_id=hyperparam_tuning_job_id,
                    row_data=dataset[k]
                )
            )
        bulk_operation = tbl_training_dataset_cache._get_collection().initialize_unordered_bulk_op()
        for row in dataset_rows:
            bulk_operation.insert(row.to_mongo())
        bulk_operation.execute()

    async def get_cached_dataset(self, account_id : str, hyperparam_tuning_job_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str

        Returns
        ----------
        dataset : list
        """
        dataset = []
        result = json.loads(tbl_training_dataset_cache.objects(account_id__exact=account_id, hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id).to_json())
        for data_row in result:
            dataset.append(
                data_row['row_data']
            )
        return dataset

    async def delete_cached_dataset(self, account_id : str, hyperparam_tuning_job_id : str) -> None:
        """
        Parameters
        ----------
        account_id : str
        hyperparam_tuning_job_id : str

        Returns
        ----------
        None
        """
        tbl_training_dataset_cache.objects(account_id__exact=account_id,hyperparam_tuning_job_id__exact=hyperparam_tuning_job_id).delete()

churnPredictionRepository = _ChurnPredictionRepository()
