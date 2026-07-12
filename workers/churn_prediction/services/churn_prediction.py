# module imports
from data.repositories.implementation.churn_repository import churnPredictionRepository
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
from datetime import datetime as dt
from operator import itemgetter
import copy


# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from sentry_sdk import capture_exception
import pandas as pd
import joblib
import numpy as np
from sklearn.preprocessing import LabelEncoder
from sklearn.cluster import KMeans
from kneed import KneeLocator

class ChainedAssignment:
    def __init__(self, chained=None):
        acceptable = [None, 'warn', 'raise']
        assert chained in acceptable, "chained must be in " + str(acceptable)
        self.swcw = chained

    def __enter__(self):
        self.saved_swcw = pd.options.mode.chained_assignment
        pd.options.mode.chained_assignment = self.swcw
        return self

    def __exit__(self, *args):
        pd.options.mode.chained_assignment = self.saved_swcw

class ChurnPredictionTraining():

    def __init__(self, account_id : str, account_type : str, account_currency : str, octy_job_id : str, bucket : str, algorithm_configurations : dict, loop : Any):
        self.account_id = account_id
        self.octy_job_id = octy_job_id
        self.bucket = bucket
        self.algorithm_configurations = algorithm_configurations
        self.loop = loop
        self.b = BillingUnits(account_id=account_id, account_type=account_type, account_currency=account_currency, process_name='churn_prediction_training', loop=loop)
        self.logger = logging.getLogger('uvicorn.error')
        self.hyperparam_tuning_job_id = generate_uid('hp-t-job')
        self.data_timeframe = Config['DATA_SET_TIMEFRAME']
        self.model_meta = {}
        self.model_meta['X_cols']="N/A"
        self.stop_list_tuple = ('has_charged',)
        self.features=[{'item_feature_list' : Config['ITEM_FEATURE_COLS']},{'profile_feature_list' : ['rfm_score', 'has_charged']}]
        self.items_df = None
        self.profiles_df = None
        self.charged_events_df = None
        self.complaints_events_df = None
        self.training_df = None
        self.profiles_ids = []
        self.csv_objects = []
        self.training_resources = []
        self.total_bytes=0
        self.mpu_upload_id = None
        self.key = None
        self.parts = None
    
    #Private Methods ***

    # Octy Job management private methods
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

    async def _dispose_job(self, ex : str) -> None:
        try:
            # Delete tuning job ref, if we have one
            await churnPredictionRepository.delete_hparam_tuning_job_ref(account_id=self.account_id, 
                                                                    hyperparam_tuning_job_id=self.hyperparam_tuning_job_id)
            # abort_multipart_upload if self.mpu_upload_id != None
            await bucketRepository.abort_multipart_upload(key=self.key,
                                                        upload_id=self.mpu_upload_id, 
                                                        bucket_name=self.bucket)
            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : f'Churn prediction training Job failed. EX :: {ex}',
                'status' : 'failed'
            })
        except Exception as err:
            self.b.complete_compute_units()
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to dispose of job. {str(err)}')

    async def _complete_job(self) -> None:
        self.logger.info('Training job complete')
        # HTTP call to confirm job completion with status
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
            'account_id' : self.account_id,
            'octy_job_id' : self.octy_job_id,
            'message' : 'Churn prediction training Job suceeded',
            'status' : 'success'
        })

        # create follow up octy job to update training job status
        self.loop.create_task(amqpPublisher.send_message(routing_key='octy.job.cmd.create',
            payload={
                'account_id' : self.account_id,
                'job_meta' : {
                    'job_type' : 'churn',
                    'amqp_routing_key': 'churn.training.complete.cmd.run',
                    'required_permissions' : ['churn'],
                    'required_configurations' :
                        { 
                            'account_attributes' : [
                                'account_configurations.webhook_url',
                                'account_configurations.account_type',
                                'account_configurations.account_currency',
                                'bucket',
                                'churn_info.churn_percentage'
                            ],
                            'algorithm_configuration_idxs' : [
                                1
                            ]
                        },
                    'desired_runs' : 1,
                    'time_interval' : 60,
                    'fail_threshold' : 3
                },
                'job_data' : {
                    'hyperparam_tuning_job_id' : self.hyperparam_tuning_job_id
                }
        }))


    # Data aggregation private methods
    async def _get_items_data(self) -> list: 
        items_data = await churnPredictionRepository.get_items(self.account_id)
        self.items_df = pd.DataFrame(items_data)
        self.items_df = self.items_df.drop(['item_category','item_name','item_description','status', 'created_at', 'updated_at'], axis = 1)
        if len(self.items_df)< Config['MIN_NUM_ITEMS']:
            raise Exception('Not enough items found to conduct model training.')

    async def _get_profiles_data(self) -> None: 
        active_profiles_data = await churnPredictionRepository.get_profiles(self.account_id, ids='false')
        churned_profiles_data = await churnPredictionRepository.get_profiles(self.account_id, status='churned', ids='false')
        profiles_data = []
        profiles_data.extend(active_profiles_data)
        profiles_data.extend(churned_profiles_data)
        self.profiles_df = pd.DataFrame(profiles_data)
        self.profiles_df['churn'] = self.profiles_df.apply(
            lambda row: True if row.status == 'churned' else False, axis=1)
        
        self.profiles_df = self.profiles_df.drop(['customer_id','rfm_score','rfm_segment_desc','churn_probability', 'status', 'created_at', 'updated_at' ], axis = 1)
        if len(self.profiles_df)< Config['MIN_NUM_PROFILES']:
            raise Exception('Not enough profiles found to conduct model training.')

    async def _get_events_data(self) -> None:
        charged_events_data = await churnPredictionRepository.get_events(self.account_id, 
                                                                self.profiles_ids, 
                                                                self.data_timeframe, 
                                                                'charged')
        complaints_events_data = await churnPredictionRepository.get_events(self.account_id, 
                                                                self.profiles_ids, 
                                                                self.data_timeframe, 
                                                                'complaint')

        if len(charged_events_data)< Config['MIN_NUM_CHARGED_EVENTS']:
            raise Exception('Not enough charged event instances found to conduct model training.')
        if len(complaints_events_data)< Config['MIN_NUM_COMPLAINTS']:
            raise Exception('Not enough complaint event instances found to conduct model training.')
        
        self.charged_events_df = pd.DataFrame(charged_events_data)
        self.complaints_events_df = pd.DataFrame(complaints_events_data)
        self.complaints_events_df = self.complaints_events_df.drop(['account_id','event_type_id','created_at','event_id'], axis = 1)
        self.charged_events_df = self.charged_events_df.drop(['account_id','event_type_id','created_at','event_id'], axis = 1)


    # Dataframe shaping & encoding private methods
    async def _get_dict_keys(self, df : pd.DataFrame, column_name : str) -> list:
        """ Take dicts from df column and return unique dict keys"""
        df_col_dicts = df[column_name].values.tolist()
        df_col_keys = [key for l in df_col_dicts for key in l.keys()]
        df_col_unique_keys = np.unique(df_col_keys).tolist()
        return df_col_unique_keys
    
    def _apply_dict_value(self, key : str, column_dict : dict) -> Any:
        """ Take dict from df column value and create new columns for each key"""
        val = np.NaN
        try:
            val = column_dict[key]
            return val
        except KeyError:
            return val

    async def _dynamic_null_drop(self) -> None:
        """ Drop null value rows OR Drop columns where % of null rows would reduce dataset size dramatically """

        cols_to_drop = []
        required_cols = copy.deepcopy(self.features[1]['profile_feature_list'])
        required_cols.extend(['segment_tags', 'profile_id', 'churn'])
        current_cols = self.profiles_df.columns.values.tolist()

        for col in current_cols:
            null_count = sum(pd.isnull(self.profiles_df[col]))
            total = len(self.profiles_df[col].values.tolist())

            # if null_count > 35% of dataset, drop column, not rows 
            null_percent = (null_count / total) * 100
            if null_percent > Config['ALLOWED_COL_NULL_COUNT']:
                cols_to_drop.append(col)

        for col in current_cols:
            if col not in self.algorithm_configurations['profile_features'] and col not in required_cols:
                cols_to_drop.append(col)

        # Drop all invalid columns
        self.logger.warning(f"Dropping 'profiles_df' columns: {cols_to_drop} due to null values exceeding {Config['ALLOWED_COL_NULL_COUNT']}% of the each columns cells values. OR columns not required for training data set.")
        self.profiles_df = self.profiles_df.drop(cols_to_drop, axis = 1)
        # Drop null rows
        self.profiles_df.dropna(how='any', axis=0, inplace=True)
        self.profiles_df.reset_index()

    async def _apply_segment_tags(self) -> None:
        """ Take dicts from profiles_df segment_tags column, deserialise tags and one hot encode segment tags for each profile"""
        segment_tags = np.unique(list(map(itemgetter('segment_tag'), \
            [val for sublist in self.profiles_df.segment_tags.values.tolist() for val in sublist]))).tolist()

        def _one_hot_encode_profile_segment_tags(tag : str, profile_tags : list) -> int:
            # Determine if tag exists in profile.
            for ptag in profile_tags:
                if ptag['segment_tag'] == tag:
                    return 1
            return 0

        for tag in segment_tags:
            seg_key = tag+"__SEGMENT"
            self.stop_list_tuple+=(seg_key,)
            self.profiles_df[seg_key] = self.profiles_df.apply(lambda row: _one_hot_encode_profile_segment_tags(tag, row['segment_tags']), axis=1)
        
        self.profiles_df = self.profiles_df.drop(['segment_tags'], axis = 1)
    
    async def _apply_total_purchase_value(self) -> None:
        purchased_items_df = pd.merge(self.charged_events_df, self.items_df, on='item_id')
        total_purchase_value_df = purchased_items_df.groupby('profile_id').item_price.sum().reset_index()
        self.training_df = pd.merge(self.training_df, total_purchase_value_df, on='profile_id', how='outer')
        self.training_df.rename(columns={"item_price": "total_purchase_value"},inplace=True)
        self.training_df["total_purchase_value"].fillna(0, inplace=True)

    async def _get_profile_most_frequent(self, events_df : pd.DataFrame, drop_columns : list, keep_col : str) -> None:
        """ Get the most frequent specified event instance attribute per profile and merge onto training_df """

        get_most_frequent = lambda values: max(Counter(values).items(), key = lambda x: x[1])[0]
        most_frequent = events_df.groupby(['profile_id']).agg(get_most_frequent)
        most_frequent = most_frequent.drop(drop_columns, axis = 1)
        self.training_df = pd.merge(self.training_df, most_frequent, on='profile_id', how='outer')
        self.training_df[keep_col].fillna('not_specified', inplace=True)

    async def _get_profile_event_count(self, events_df : pd.DataFrame, drop_columns : list, new_columns : list) -> None:
        """ Get the count of specified event instance types (in events_df) per profile and merge onto training_df """

        total_event_count = events_df.groupby('profile_id').count().reset_index()
        total_event_count = total_event_count.drop(drop_columns, axis=1)
        # NOTE: events can carry an arbitrary number of dynamic 'event_properties' derived
        # columns (see _build_training_dataset), so more than just 'profile_id' and
        # 'event_type' may remain here. Only 'profile_id' and 'event_type' (a required,
        # always-populated field on every event) are needed for the count, so restrict to
        # those before renaming to avoid a column count mismatch on rename.
        total_event_count = total_event_count[['profile_id', 'event_type']]
        total_event_count.columns = new_columns
        self.training_df = pd.merge(self.training_df, total_event_count, on='profile_id', how='outer')
        self.training_df[new_columns[1]].fillna(0, inplace=True)
        self.training_df[new_columns[1]] = self.training_df[new_columns[1]].astype(int)

    async def _identify_drop_invalid_numerical_columns(self) -> Union[list, list]:
        """ Identify all numerical columns in training_df. 
        Any numerical columns with less than x unique values will be dropped """
        # num_cluster_cols (columns that will be numerically cluster encoded. > 10 unique observations)
        # num_bin_cols (columns that will be seperated by dynamic bin categorical limits and one hot encoded. > 2 and < 10 unique observations)
        num_cluster_cols = []
        num_bin_cols = []
        num_cols = self.training_df.select_dtypes(include=np.number).columns.tolist()
        if self.stop_list_tuple != None:
            num_cols = [e for e in num_cols if e not in self.stop_list_tuple]
        
        for n_col in num_cols:

            unique_observations = len(pd.unique(self.training_df[n_col]))

            if unique_observations < 2:
                self.logger.warning(f"Dropping numerical column: {n_col} due to insufficient number of unique values")
                self.training_df = self.training_df.drop([n_col], axis = 1)
                continue
            
            if unique_observations < 10:
                num_bin_cols.append(n_col)
            else:
                num_cluster_cols.append(n_col)

        self.training_df.reset_index()

        return num_cluster_cols, num_bin_cols

    async def _numerical_quantile_bin_encoding(self, feature_name : str, quantiles : int = 3) -> None:
        quantile_field_name = feature_name + '_quantiles'
        self.logger.info(f"Creating {quantiles} quantile bins for column {feature_name}")
        self.training_df[quantile_field_name] = \
            pd.qcut(self.training_df[feature_name], q=quantiles) #, duplicates='drop'
        self.logger.info(f"One hot encoding {quantile_field_name}")
        self.training_df = pd.concat([self.training_df,pd.get_dummies(self.training_df[quantile_field_name], prefix=feature_name)],axis=1)
        self.training_df = self.training_df.drop([quantile_field_name, feature_name], axis = 1)

        self.logger.info("=============== END QUANTILE BIN FUNC ===============")

    async def _numerical_bin_encoding(self, feature_name : str, bins : int = 3) -> None:
        bin_field_name = feature_name + '_bins'
        self.logger.info(f"Creating {bins} bins for column {feature_name}")
        self.training_df[bin_field_name] = \
            pd.cut(self.training_df[feature_name], bins)
        self.logger.info(f"One hot encoding {bin_field_name}")
        self.training_df = pd.concat([self.training_df,pd.get_dummies(self.training_df[bin_field_name], prefix=feature_name)],axis=1)
        self.training_df = self.training_df.drop([bin_field_name, feature_name], axis = 1)

        self.logger.info("=============== END BIN FUNC ===============")

    async def _format_column_names(self):
        self.logger.info("Formatting column names, replacing illegal characters, to meet required conventions")
        # format one hot encoded quantile_bin encoded column names
        col_dict = {}
        for col in self.training_df.columns:
            og_col_name = col
            did_f = False
            for ch in ['\\','`','*','_','{','}','[',']','(',')','>','#','+','-','.','!','$','\'', ',', ' ']:
                if ch in col:
                    did_f = True
                    col = col.replace(ch,"_")
            if did_f:
                col_dict[og_col_name] = col
        
        self.logger.info(f"Formatting column names : {col_dict}")

        self.training_df = self.training_df.rename(columns=col_dict)

    async def _numerical_cluster_encoding(self, feature_name : str, ascending : bool) -> None:
        #programmatically determine "elbow" (optimum number of clusters)
        cluster_errors = [] #create array to hold errors
        df_cluster = self.training_df[[feature_name]] #isolate feature column data

        observations = len(df_cluster) #if observations < 30, drop column from dataframe and return.
        if observations < 30:
            return
        
        #Get number of unique values in feature column 
        len_unique=len(self.training_df[feature_name].unique())
        self.logger.info("Number of unique values in column {c} : {n}".format(c=feature_name,n=len_unique))
        if len_unique < 10:
            cluster_range = range(1, len_unique)
        else:
            cluster_range = range(1, 10) #test for cluster sizes 1 to 10

        for num_clusters in cluster_range:
            kmeans = KMeans(n_clusters=num_clusters,max_iter=1000,init='k-means++',random_state=42).fit(df_cluster)
            cluster_errors.append( kmeans.inertia_ )

        clusters_df = pd.DataFrame({"cluster_errors": cluster_errors, "num_clusters": cluster_range})
        
        elbow = KneeLocator(clusters_df.num_clusters.values, clusters_df.cluster_errors.values, S=1.0, curve='convex', direction='decreasing')
        knee = elbow.knee
        self.logger.info("Original knee: {}".format(str(knee)))
        if knee > 5:
            self.logger.info("Limited knee to : 5")
            knee = 5 #limit knee to 5 max
        self.logger.info(feature_name+':')
        self.logger.info('creating a K-means cluster with ' + str(knee) + ' clusters')
        
        #cluster feature_name data
        cluster_field_name = feature_name + '_cluster'
        kmeans = KMeans(n_clusters=knee)
        kmeans.fit(self.training_df[[feature_name]])
        #create feature_name_cluster column in dataframe
        with ChainedAssignment():
            self.training_df[cluster_field_name] = kmeans.predict(self.training_df[[feature_name]])
        self.logger.info('CREATED a K-means cluster with ' + str(knee) + ' clusters')


        #convert cluster number to categorical label
        if knee == 2:
            label_map = {0: 'low', 1: 'high'}
        elif knee == 3:
            label_map = {0: 'low', 1: 'mid', 2: 'high'}
        elif knee == 4:
            label_map = {0: 'low', 1: 'mid', 2: 'mid-high', 3: 'high'}
        elif knee == 5:
            label_map = {0: 'low', 1: 'mid', 2: 'mid-high', 3: 'high', 4: 'top-high'}
        
        #re order dataframe
        df_new = self.training_df.groupby(cluster_field_name)[feature_name].mean().reset_index()
        df_new = df_new.sort_values(by=feature_name,ascending=ascending).reset_index(drop=True)
        df_new['index'] = df_new.index
        self.training_df = pd.merge(self.training_df,df_new[[cluster_field_name,'index']], on=cluster_field_name)
        self.training_df = self.training_df.drop([feature_name],axis=1)
        self.training_df = self.training_df.drop([cluster_field_name],axis=1)
        self.training_df = self.training_df.rename(columns={"index":cluster_field_name})
        #add categorical labels to cluster numbers
        self.training_df[cluster_field_name] = self.training_df[cluster_field_name].replace(label_map)

        self.logger.info("=============== END CLUSTER FUNC ===============")

    async def _categorical_encoding(self, stop_list : list) -> None:
        le = LabelEncoder()
        dummy_columns = []
        for column in self.training_df.columns:
            if self.training_df[column].dtype == object and column not in stop_list:
                # if self.training_df[column].nunique() == 2:
                #     self.training_df[column] = le.fit_transform(self.training_df[column]) 
                # else:
                #     dummy_columns.append(column)
                dummy_columns.append(column)
        self.training_df = pd.get_dummies(data=self.training_df,columns=dummy_columns)


    # Dataset file private methods
    async def _dataframe_to_csv_obj(self, dataframe : object, type_ : str) -> dict:
        return  {
            'data' : dataframe.to_csv(index=False),
            'type' : type_
        }
 
    async def _create_csv_objects(self) -> None:
        # convert df to csv and store in csv_objects array
        training_csv = await self._dataframe_to_csv_obj(self.training_df, 'training')
        self.csv_objects.extend([training_csv])

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


    # Training Job private methods
    async def _create_hparam_tuning_job_ref(self) -> None:
        meta_data = {'event_type' : self.algorithm_configurations['event_type'], 'features' : self.features}
        await churnPredictionRepository.create_hparam_tuning_job_ref(hyperparam_tuning_job_id=self.hyperparam_tuning_job_id,
                                                                account_id=self.account_id,
                                                                meta_data=meta_data)
    
    async def _cache_dataset(self) -> None:
        # convert training df to json
        dataset_json = json.loads(self.training_df.to_json(orient='index'))
        await churnPredictionRepository.cache_dataset(self.account_id,self.hyperparam_tuning_job_id, dataset_json)


    #Hyper parameter tuning job private methods ***

    # Dataframe creation public methods
    async def _build_training_dataset(self) -> None:
        self.logger.info('Building training dataset ...')

        # Build items dataframe
        self.logger.info('Building items dataframe ...')
        await self._get_items_data()
        self.logger.info('Built items dataframe')

        # Build profiles dataframe

        # Get profiles data
        self.logger.info('Building profiles dataframe ...')
        await self._get_profiles_data()

        # Create feature columns
        platform_info_unique_keys = await self._get_dict_keys(self.profiles_df, 'platform_info')
        profile_data_unique_keys = await self._get_dict_keys(self.profiles_df, 'profile_data')
        for key in platform_info_unique_keys:
            if key in self.algorithm_configurations['profile_features']:
                self.profiles_df[key] = self.profiles_df.apply(lambda row: self._apply_dict_value(key, row['platform_info']), axis=1)
        for key in profile_data_unique_keys:
            if key in self.algorithm_configurations['profile_features']:
                self.profiles_df[key] = self.profiles_df.apply(lambda row: self._apply_dict_value(key, row['profile_data']), axis=1)
        
        # Shape dataframe based on null values
        await self._dynamic_null_drop()

        # Apply and encode sement tags
        await self._apply_segment_tags()

        self.profiles_df.reset_index(drop=True)

        # Get profile IDS for other data requests
        self.profiles_ids = self.profiles_df.profile_id.values.tolist()

        self.logger.info('Built profiles dataframe')

        # Build training data dataframe
        self.logger.info('Building training dataframe ...')

        self.training_df = self.profiles_df.copy(deep=True)

        await self._get_events_data()
        charged_event_properties_unique_keys = await self._get_dict_keys(self.charged_events_df, 'event_properties')
        complaints_event_properties_unique_keys = await self._get_dict_keys(self.complaints_events_df, 'event_properties')
        for key in charged_event_properties_unique_keys:
            self.charged_events_df[key] = self.charged_events_df.apply(lambda row: self._apply_dict_value(key, row['event_properties']), axis=1)
        for key in complaints_event_properties_unique_keys:
            self.complaints_events_df[key] = self.complaints_events_df.apply(lambda row: self._apply_dict_value(key, row['event_properties']), axis=1)

        self.charged_events_df = self.charged_events_df.drop(['event_properties'], axis = 1)
        self.complaints_events_df = self.complaints_events_df.drop(['event_properties'], axis = 1)
        
        # Get total purchase value for each profile
        await self._apply_total_purchase_value()

        # Payment method and complaint channel frequencies
        await self._get_profile_most_frequent(self.charged_events_df, ['item_id','event_type' ], 'payment_method')
        await self._get_profile_most_frequent(self.complaints_events_df, ['event_type'], 'channel')

        # Number of charges & Number of complaints
        await self._get_profile_event_count(self.charged_events_df, ['item_id','payment_method'], ['profile_id','number_charges'])
        await self._get_profile_event_count(self.complaints_events_df, ['channel'], ['profile_id','number_complaints'])

        # Encode training dataframe 
        num_cluster_cols, num_bin_cols = await self._identify_drop_invalid_numerical_columns()
        for col in num_cluster_cols:
            await self._numerical_cluster_encoding(feature_name=col, ascending=True)
        for col in num_bin_cols:
            await self._numerical_bin_encoding(feature_name=col)
        
        await self._categorical_encoding(['profile_id'])

        if len(self.training_df) < Config["MIN_NUM_ROWS_COLLECTIVE"]:
            raise Exception('Not enough valid data to conduct model training.')
        
        await self._format_column_names()
        
        #Update profile feature columns 
        X = self.training_df.drop(['churn', 'profile_id'],axis=1)
        self.features[1]['profile_feature_list'] = list(X.columns)


    # Dataset file public methods
    async def _upload_resources(self) -> None:
        self.logger.info('Uploading training job resources')

        await self._create_csv_objects()

        for csv_object in self.csv_objects:
            self.b.track_data_units(csv_object)
            #determine size of file
            file_size = sys.getsizeof(csv_object['data'])
            if file_size  < int(Config['MIN_FILE_SIZE']):
                self.key = await bucketRepository.single_upload(file_data=csv_object['data'],
                                                    resource_friendly_name=csv_object['type'],
                                                    hyperparam_tuning_job_id=self.hyperparam_tuning_job_id,
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
                                                                    hyperparam_tuning_job_id=self.hyperparam_tuning_job_id,
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

    # Training Job public methods
    async def _start_cloud_hparam_tuning_job(self) -> None:
        self.logger.info('Starting cloud hyper parameter tuning job...')
        # get parent hyper parameter tuning job for warm start
        parent_job = await churnPredictionRepository\
            .get_parent_hparam_tuning_job_ref(account_id=self.account_id)
        parent_job_id = None
        if parent_job:
            parent_job_id = parent_job['_id']

        #calculate required volume size
        volume_size = await self._required_gb(self.total_bytes)
        await churnPredictionRepository.start_hparam_tuning_job(account_id=self.account_id, 
                                                            hyperparam_tuning_job_id=self.hyperparam_tuning_job_id, 
                                                            parent_hyperparam_tuning_job_id=parent_job_id,
                                                            volume_size=volume_size, 
                                                            training_resources=self.training_resources,
                                                            bucket_name=self.bucket)
        self.logger.info('Cloud hyper parameter tuning job started!')

        # create DB ref
        await self._create_hparam_tuning_job_ref()

        # cache dataset
        await self._cache_dataset()
    
    # Entry point
    async def run(self) -> None: 
        try:
            self.b.track_compute_units('hours')
            # Build training data sets
            await self._build_training_dataset()

            #upload training data resources 
            await self._upload_resources()

            # begin cloud training
            await self._start_cloud_hparam_tuning_job()
  
            await self._complete_job()
            self.b.complete_compute_units()

        except Exception as e:
            capture_exception(e)
            self.logger.critical(str(e))
            self.b.complete_compute_units()
            await self._dispose_job(ex=str(e))


class ChurnPredictionCompleteTrainingJob():

    def __init__(self, account_id : str, 
                account_type : str, 
                account_currency : str,
                octy_job_id : str, 
                bucket : str, 
                hyperparam_tuning_job_id : str, 
                previous_churn_percentage : int,
                algorithm_configurations : dict, 
                webhook_url : str,
                loop : Any):

        self.account_id = account_id
        self.octy_job_id = octy_job_id
        self.bucket = bucket
        self.algorithm_configurations = algorithm_configurations
        self.hyperparam_tuning_job_id = hyperparam_tuning_job_id
        self.loop = loop
        self.b = BillingUnits(account_id=account_id, account_type=account_type, account_currency=account_currency, process_name='churn_prediction_completion', loop=loop)
        self.best_training_job = None
        self.hp_tuning_job = None
        self.webhook_url = webhook_url
        self.previous_churn_percentage = previous_churn_percentage
        self.logger = logging.getLogger('uvicorn.error')
        self.amqp_message_size_limit = 104857600 #100 MB AMQP message limit
        self.model_meta = None
        self.trained_model = None
        self.features = None
        self.current_churn = None
        self.cached_df = None
        self.X_pred_cols = None
        self.predictions_df = None
        self.training_compute_units = 0

    
    async def _get_cloud_hp_tuning_status(self) -> str:
        self.logger.info('Getting cloud training status')
        status = await churnPredictionRepository.get_hparam_tuning_job_status(hyperparam_tuning_job_id=self.hyperparam_tuning_job_id)
        return status
    
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


    async def _job_failed_webhook(self) -> None:
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has failed.',
            'body' : {
                'algorithm' : 'churn-prediction',
                'job_status' : 'Failed',
                'message' : 'An issue occurred when attempting to train a churn prediction model. You do have to do anything as our systems will rectify this issue automatically. If this issue repeatedly occurs, please contact the Octy support team: support@octy.ai'
            },
            'date_time' : str(dt.now())
        })
    
    async def _job_success(self) -> None:
        await churnPredictionRepository.update_hparam_tuning_job_ref(account_id=self.account_id,
                                                    hyperparam_tuning_job_id=self.hyperparam_tuning_job_id,
                                                    best_model_training_job_id=self.best_training_job['training_job_name'],
                                                    model_meta=self.model_meta,
                                                    status=self.status)
        await self._send_http_account_webhook_request(self.webhook_url, {
            'subject' : 'Octy training job has successfully completed.',
            'body' : {
                'algorithm' : 'churn-prediction',
                'job_status' : 'Completed',
                'message' : 'This means a new, up to date churn prediction analysis report is avilable and updated churn predictions have been applied to each profile. Octy support team.'
            },
            'date_time' : str(dt.now())
        })
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'Churn prediction training Job successfully completed',
                'status' : 'success'
        })

    async def _re_schedule_job(self) -> None:
        self.logger.info('Rescheduling job')
        # job is still processing
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : 'Churn-prediction training Job still processing',
                'status' : 'failed' # send failed so next octy job tick can re-run it.
        })
        
    async def _destroy_job(self) -> None:
        try:
            self.logger.warning('Destroying job due to error')
            # delete training job artefacts 
            await bucketRepository.delete_directory(bucket_name=self.bucket, 
                                                    directory_path=f"{Config['CHURN_PRED_MODELS_DIR']}/{self.hyperparam_tuning_job_id}")
            # Update tuning job reference to 'failed'
            await churnPredictionRepository.update_hparam_tuning_job_ref(account_id=self.account_id,
                                                                hyperparam_tuning_job_id=self.hyperparam_tuning_job_id,
                                                                best_model_training_job_id='--',
                                                                status='Failed',
                                                                model_meta=self.model_meta)
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

        self.hp_tuning_job = await churnPredictionRepository.get_hparam_tuning_job_ref(account_id=self.account_id, \
            hyperparam_tuning_job_id=self.hyperparam_tuning_job_id, status='in_progress')
        
        # get best job
        self.best_training_job, self.training_compute_units = await churnPredictionRepository\
            .get_best_training_job(hyperparam_tuning_job_id=self.hyperparam_tuning_job_id)

        
        model_location =  f"{Config['CHURN_PRED_MODELS_DIR']}/{self.best_training_job['training_job_name']}/output/model.tar.gz"
        files = await bucketRepository.download_resource(bucket_name=self.bucket,
                                                        key=model_location,
                                                        is_compressed=True)
        for file_bytes in files:
            if file_bytes['file_name'] == 'model_meta_data.json':
                self.model_meta = json.loads(file_bytes['file_data'].read())
            elif file_bytes['file_name'] == 'trained_churn_prediction_model.pkl':
                self.trained_model = joblib.load(BytesIO(file_bytes['file_data'].read()))
            elif file_bytes['file_name'] == 'features.pkl':
                self.features = joblib.load(BytesIO(file_bytes['file_data'].read()))
            elif file_bytes['file_name'] == 'current_churn.pkl':
                self.current_churn = joblib.load(BytesIO(file_bytes['file_data'].read()))

    async def _churn_calculations(self) -> None:
        churn_diff = round(self.previous_churn_percentage - self.current_churn, 1)
        if churn_diff > 0:
            churn_indicator='positive'
        elif churn_diff < 0:
            churn_indicator='negative'
        elif churn_diff == 0.0:
            churn_indicator='stalled'

        # NOTE: Destroy job if features contains NaN values,
        # this signifies that the model is overfitted.
        feature_vals = [d['feature_importance'] for d in self.features]
        if 'NaN' in feature_vals or np.NaN in feature_vals:
            await self._destroy_job()

        # NOTE: Destroy job if features is > 70% dominant in features list,
        # this signifies that the model is overfitted.
        def CountFrequency(l):
            count = {}
            for i in l:
                count[i] = count.get(i, 0) + 1
            return count


        for _, v in CountFrequency(feature_vals).items():
            if (v*100)/len(feature_vals) > 70.0:
                await self._destroy_job()

        # update account churn report information
        self.loop.create_task(amqpPublisher.send_message(routing_key='churn.info.cmd.update',
            payload={
                'account_id' : self.account_id,
                'churn_info' : {
                    'churn_percentage' : self.current_churn,
                    'churn_indicator' : churn_indicator,
                    'churn_difference' : churn_diff,
                    'features' : self.features
                }  
            }))

    async def _get_cached_dataset(self) -> None:
        self.logger.info('Loading cached dataset...')
        cached_dicts = await churnPredictionRepository.get_cached_dataset(account_id=self.account_id, \
            hyperparam_tuning_job_id=self.hyperparam_tuning_job_id)
        self.cached_df = pd.DataFrame(cached_dicts)
        self.logger.info('Cached dataset loaded!')

    async def _destroy_dataset_cache(self) -> None:
        self.prediction_df = await churnPredictionRepository.delete_cached_dataset(account_id=self.account_id, \
            hyperparam_tuning_job_id=self.hyperparam_tuning_job_id)
        self.logger.info('Cached dataset Deleted!')

    async def _get_feature_columns(self) -> None:
        X_pred = self.cached_df.drop(['churn', 'profile_id'],axis=1)
        self.X_pred_cols=list(X_pred.columns)
    
    async def _numerical_clustering_encoding(self, feature_name : str, ascending : bool) -> None:
        #programmatically determine "elbow" (optimum number of clusters)
        cluster_errors = [] #create array to hold errors
        df_cluster = self.predictions_df[[feature_name]] #isolate feature column data

        observations = len(df_cluster) #if observations < 30, drop column from dataframe and return.
        if observations < 30:
            return
        
        #Get number of unique values in feature column 
        len_unique=len(self.predictions_df[feature_name].unique())
        self.logger.info("Number of unique values in column {c} : {n}".format(c=feature_name,n=len_unique))
        if len_unique < 10:
            cluster_range = range(1, len_unique)
        else:
            cluster_range = range(1, 10) #test for cluster sizes 1 to 10

        for num_clusters in cluster_range:
            kmeans = KMeans(n_clusters=num_clusters,max_iter=1000,init='k-means++',random_state=42).fit(df_cluster)
            cluster_errors.append( kmeans.inertia_ )

        clusters_df = pd.DataFrame({"cluster_errors": cluster_errors, "num_clusters": cluster_range})
        
        elbow = KneeLocator(clusters_df.num_clusters.values, clusters_df.cluster_errors.values, S=1.0, curve='convex', direction='decreasing')
        knee = elbow.knee
        self.logger.info("OG knee: {}".format(str(knee)))
        if knee > 5:
            knee = 5 #limit knee to 5 max
        self.logger.info(feature_name+':')
        self.logger.info('creating a K-means cluster with ' + str(knee) + ' clusters')
        
        #cluster feature_name data
        cluster_field_name = feature_name + '_cluster'
        kmeans = KMeans(n_clusters=knee)
        kmeans.fit(self.predictions_df[[feature_name]])
        #create feature_name_cluster column in dataframe
        with ChainedAssignment():
            self.predictions_df[cluster_field_name] = kmeans.predict(self.predictions_df[[feature_name]])
        self.logger.info('CREATED a K-means cluster with ' + str(knee) + ' clusters')


        #convert cluster number to categorical label
        if knee == 2:
            label_map = {0: 'low', 1: 'high'}
        elif knee == 3:
            label_map = {0: 'low', 1: 'mid', 2: 'high'}
        elif knee == 4:
            label_map = {0: 'low', 1: 'mid', 2: 'mid-high', 3: 'high'}
        elif knee == 5:
            label_map = {0: 'low', 1: 'mid', 2: 'mid-high', 3: 'high', 4: 'top-high'}
        
        #re order dataframe
        df_new = self.predictions_df.groupby(cluster_field_name)[feature_name].mean().reset_index()
        df_new = df_new.sort_values(by=feature_name,ascending=ascending).reset_index(drop=True)
        df_new['index'] = df_new.index
        self.predictions_df = pd.merge(self.predictions_df,df_new[[cluster_field_name,'index']], on=cluster_field_name)
        self.predictions_df = self.predictions_df.drop([feature_name],axis=1)
        self.predictions_df = self.predictions_df.drop([cluster_field_name],axis=1)
        self.predictions_df = self.predictions_df.rename(columns={"index":cluster_field_name})
        #add categorical labels to cluster numbers
        self.predictions_df[cluster_field_name] = self.predictions_df[cluster_field_name].replace(label_map)

        self.logger.info("=============== END CLUSTER FUNC ===============")

    async def _predict_churn_scores(self) -> None:
        self.logger.info('Generating churn prediction scores')

        await self._get_feature_columns()
        with ChainedAssignment():
            self.cached_df['churn_prob'] = self.trained_model.predict_proba(self.cached_df[self.X_pred_cols])[:,1]

        self.predictions_df = self.cached_df[['profile_id', 'churn_prob']]
        
        if self.predictions_df['churn_prob'].nunique() < 5:
            # NOTE: Do not apply predictions where predictions are identical across all profiles,
            # this indicates that the model is overfitted.
            # In this case, send callback to octy job service to destroy job.
            await self._destroy_job()

        await self._numerical_clustering_encoding('churn_prob', True)

    async def _assign_churn_scores(self) -> None:
        self.logger.info('Assigning churn prediction scores to profiles')

        # NOTE: _numerical_clustering_encoding skips clustering (and never creates
        # 'churn_prob_cluster') when there are fewer than 30 predictions to cluster.
        # In that case, fall back to a single default label for all profiles instead
        # of crashing with a KeyError below.
        if 'churn_prob_cluster' not in self.predictions_df.columns:
            self.predictions_df['churn_prob_cluster'] = 'not_specified'

        amqp_batch_profiles = []
        # convert predictions_df to list of dictonaries
        predictions_dicts = self.predictions_df.to_dict('records')
        profile_updates = []
        for pd in predictions_dicts:

            profile_updates.append(
                {
                    'profile_id' : pd['profile_id'],
                    'churn_probability' : pd['churn_prob_cluster']
                }
            )

            # split profile objects according to AMQP message size limit.
            if sys.getsizeof(profile_updates) > self.amqp_message_size_limit:
                amqp_batch_profiles.append(profile_updates)
                # flush profile_updates
                profile_updates = []

        # flush any remaining profile_updates that never hit the size limit
        # (this also covers the case where the limit was never exceeded at all)
        if profile_updates:
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
            self.status = await self._get_cloud_hp_tuning_status()

            self.logger.info(f'Job ID : {self.hyperparam_tuning_job_id} -- Status : {self.status}')

            if self.status == 'InProgress':
                await self._re_schedule_job()
            
            elif self.status == 'Completed':
                self.b.track_compute_units('hours')
                await self._get_job_artifacts()
                await self._churn_calculations()
                await self._get_cached_dataset()
                await self._predict_churn_scores()
                await self._assign_churn_scores()
                await self._destroy_dataset_cache()
                await self._job_success()
                self.b.complete_compute_units(additional_unit_hours=self.training_compute_units)

            elif self.status == 'Failed':
                await self._destroy_job()

            elif self.status == 'Stopping':
                await self._destroy_job()

            elif self.status == 'Stopped':
                await self._destroy_job()
            
            self.logger.info(f'Completed Job!')

        except Exception as e:
            self.logger.critical(str(e))
            capture_exception(e)
            self.b.complete_compute_units(additional_unit_hours=self.training_compute_units)
            try:
                await self._re_schedule_job()
            except Exception as reschedule_err:
                capture_exception(reschedule_err)
                self.logger.critical(f'Error occurred when attempting to re-schedule job. {str(reschedule_err)}')