# module imports
from data.repositories.implementation.profiles_iden_repository import profilesIdenRepository
from .billing import BillingUnits
from utils.utils import *
from config import Config

# python imports
import logging
import time
import json
from datetime import datetime as dt
from datetime import timedelta as td 
import itertools
import sys
from typing import Any

# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from sentry_sdk import capture_exception
import pandas as pd
import numpy as np


class ProfileIdentification():
    """
        ProfileIdentification
        Handles:
        - Identifying & Merging authenticated and anonymous profiles (profiles creating during unauthenticated sessions).
        ...
    """
    def __init__(self, account_id : str, webhook_url : str, account_type : str, account_currency : str, authenticated_id_key : str,  octy_job_id : str, loop : Any): 
        self.account_id = account_id
        self.webhook_url = webhook_url
        self.authenticated_id_key = authenticated_id_key
        self.octy_job_id = octy_job_id
        self.loop = loop
        self.b = BillingUnits(account_id=account_id,account_type=account_type, account_currency=account_currency, process_name='profile_identification', loop=loop)
        self.logger = logging.getLogger('uvicorn.error')
        self.profiles = list()
        self.profiles_df = None
        self.group_profiles_df = None
        self.parent_profiles_df = None
        self.profiles_batch = None
        self.time_score_map = {'1' : dt.now() - td(days=90), 
            '2' : dt.now() - td(days=60), 
            '3' : dt.now() - td(days=30), 
            '2*' : dt.now() - td(days=20), 
            '1*' : dt.now() - td(days=10)}
        self.group_profile_dicts = None
        self.parent_profile_updates = list()
        self.amqp_messages = [
            {
                'type' : 'event_instance_profiles',
                'key' : 'profiles',
                'routing_key' : 'events.cmd.update',
                'messages': []
            },
            {
                'type' : 'rec_cache_delete',
                'key' : 'profiles',
                'routing_key' : 'reccache.cmd.delete',
                'messages' : []
            },
            {
                'type' : 'profiles',
                'key' : 'profiles',
                'routing_key' : 'profiles.cmd.update',
                'messages': []
            },
            {
                'type' : 'profiles_delete',
                'key' : 'profiles',
                'routing_key' : 'profiles.cmd.delete',
                'messages' : []
            },
            {
                'type' : 'past_segment_profiles',
                'key' : 'profiles',
                'routing_key' : 'segment.profiles.cmd.update',
                'messages' : []
            }
        ]
        self.amqp_message_size_limit = 104857600 #100 MB
        self.webhook_payload_size_limit = 104857600 #100 MB

    #Private Methods ***

    # Octy Job management private methods

    async def _append_message_payload(self, message_body : dict, type_ : str) -> None:
        mes = next(m for m in self.amqp_messages if m["type"] == type_)
        mes['messages'].append(message_body)

    async def _process_amqp_messages(self) -> None:
        
        def chunks(l : list, n : int):
            for i in range(0, n):
                yield l[i::n]

        for mes in self.amqp_messages:
            if sys.getsizeof(mes['messages']) < self.amqp_message_size_limit:
                self.loop.create_task(amqpPublisher.send_message(routing_key=mes['routing_key'],
                    payload={'account_id' : self.account_id, mes['key'] : mes['messages']}))
            else:
                for chunk in list(chunks(mes['messages'], round(sys.getsizeof(mes['messages']) / self.amqp_message_size_limit))):
                    self.loop.create_task(amqpPublisher.send_message(routing_key=mes['routing_key'],
                        payload={'account_id' : self.account_id, mes['key'] : chunk}))

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
            self.logger.info(f'Took {t1 - t0}seconds')

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

    async def _dispose_job(self, ex : str) -> None:
        try:
            # HTTP call to confirm job completion with status
            await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
                'account_id' : self.account_id,
                'octy_job_id' : self.octy_job_id,
                'message' : f'Profile identification job failed. EX :: {ex}',
                'status' : 'failed'
            })
            self.b.complete_compute_units()
        except Exception as err:
            self.b.complete_compute_units()
            capture_exception(err)
            self.logger.critical(f'Error occurred when attempting to dispose of job. {err}')

        raise Exception(ex)

    async def _complete_job(self) -> None:

        def chunks(l : list, n : int):
            for i in range(0, n):
                yield l[i::n]

        dropped_account_profiles = [{k: v for k, v in d.items() if k != 'account_id'} for d in self.profiles_batch]

        if sys.getsizeof(dropped_account_profiles) < self.webhook_payload_size_limit :
                await self._send_http_account_webhook_request(
                    url=self.webhook_url,
                    payload={
                        'subject' : 'Profile identification service output',
                        'body' : {
                            'profiles' : dropped_account_profiles
                        },
                        'date_time' : str(dt.now())
                    }
                )
        else:
            for chunk in list(chunks(dropped_account_profiles, round(sys.getsizeof(dropped_account_profiles) / self.webhook_payload_size_limit ))):
                await self._send_http_account_webhook_request(
                    url=self.webhook_url,
                    payload={
                        'subject' : 'Profile identification service output',
                        'body' : {
                            'profiles' : chunk
                        },
                        'date_time' : str(dt.now())
                    }
                )

        self.logger.info('Successfully completed profile identification job!')
        # HTTP call to confirm job completion with status
        await self._send_http_request(Config['OCTY_JOB_SERVICE_CLUSTER_IP']+'/v1/internal/jobs/callback', {
            'account_id' : self.account_id,
            'octy_job_id' : self.octy_job_id,
            'message' : 'Profile identification job completed successfully',
            'status' : 'success'
        })

    # Data aggregation private methods
    async def _build_profiles_df(self) -> None: 
        active_profiles_data = await profilesIdenRepository.get_profiles(self.account_id, ids='false')
        churned_profiles_data = await profilesIdenRepository.get_profiles(self.account_id, status='churned', ids='false')
        self.profiles.extend(active_profiles_data)
        self.profiles.extend(churned_profiles_data)
        # drop profiles that do not have self.authenticated_id_key set in their profile_data attribute
        self.profiles[:] = [d for d in self.profiles if d['profile_data'].get(self.authenticated_id_key)]
        if len(self.profiles)< 3:
            await self._dispose_job(ex=str(Exception(f"Less than three profiles were found with the set authenticated_id_key : {self.authenticated_id_key} set in their profile_data attribute.")))
        self.profiles_df = pd.json_normalize(self.profiles)

    # Dataframe shaping private methods
    async def _group_profiles(self) -> None:
        grouped = self.profiles_df.groupby("profile_data."+self.authenticated_id_key)
        self.group_profiles_df = grouped["profile_id"].apply(list)
        self.group_profiles_df = self.group_profiles_df.reset_index()

    def _score_profile(self, profile) -> int: 
        '''Score profile based on attributes. '''
        score = 0

        # prediction attributes. 
        # If these attributes are set, it means enough data was available for this profile to complete analysis jobs.
        if profile['rfm_score'] != None:
            score += 1
        if profile['churn_probability'] != None:
            score += 1

        # An active profile is more valuable than a churned profile. 
        # This attribute is therefore weighted heavily if value is 'active'.
        if profile['status'] == 'active':
            score += 5
        
        # created at. Profiles that are mature, but not too mature, have a greater probability of being relevant.
        for k, v in self.time_score_map.items():
            if str_to_dt(profile['created_at']) > v:
                continue
            else:
                k = k.replace('*', '')
                score += int(k) # apply score specified in map

        # updated at. Shows recent activity on this profile.
        delta = dt.now() - str_to_dt(profile['updated_at'])
        if delta.days <= 5:
            score += 5
        elif delta.days > 5 and delta.days <= 10:
            score += 4
        elif delta.days > 10 and delta.days <= 30:
            score += 3
        elif delta.days > 30 and delta.days <= 90:
            score += 2
        elif delta.days > 90:
            score += 1

        # number of segment tags. This indicates the number of events that has occurred for this profile.
        active_segment_tag_count = 0
        for tag in profile['segment_tags']:
            if tag['status'] == 'active':
                active_segment_tag_count += 1

        if active_segment_tag_count > 0 and active_segment_tag_count < 3:
            score += 1
        elif active_segment_tag_count >= 3 and active_segment_tag_count < 8:
            score += 2
        elif active_segment_tag_count >= 8 and active_segment_tag_count < 15:
            score += 3
        elif active_segment_tag_count >= 15:
            score += 4
        
        return score

    def _apply_scores(self, profile_ids : list) -> None:
        scores = {}
        for id_ in profile_ids:
            scores[id_] = self._score_profile(next(p for p in self.profiles if p["profile_id"] == id_))
        return scores

    def _select_parent_profile(self, scores : dict) -> str:
        return next(iter(sorted(scores, key=scores.get, reverse=True)))

    def _specify_child_profile_count(self, parent_profile : str, child_profiles : list) -> int:
        try:
            child_profiles.remove(parent_profile)
        except ValueError:
            pass
        return len(child_profiles)

    def _specify_child_profiles(self, parent_profile : str, child_profiles : list) -> list:
        try:
            child_profiles.remove(parent_profile)
        except ValueError:
            pass
        return child_profiles

    async def _drop_null_child_profiles(self) -> None:
        zero_children = self.group_profiles_df[self.group_profiles_df["child_profile_count"] == 0]
        zero_children_profile_ids = zero_children['parent_profile'].to_list()
        self.profiles = [profile for profile in self.profiles if profile['profile_id'] not in zero_children_profile_ids]
        self.group_profiles_df.drop(self.group_profiles_df.index[self.group_profiles_df['child_profile_count'] == 0], inplace = True)


    async def _merge_segment_tags(self) -> None:
        def _append_tag(tag):
            if tag['segment_id'] not in segment_ids and tag['status'] == 'active':
                segment_ids.append(tag['segment_id'])
                segment_tags.append(
                    {
                        'segment_id': tag['segment_id'],
                        'segment_tag': tag['segment_tag'],
                        'status': 'active',
                    })

        for profile in self.group_profile_dicts:
            parent_profile = next(p for p in self.profiles if p["profile_id"] == profile['parent_profile'])
            segment_ids = list()
            segment_tags = list()

            # merge all segment tags
            for tag in parent_profile['segment_tags']:
                if tag not in segment_tags and tag['status'] == 'active':
                    _append_tag(tag)
            for child in profile['child_profiles']:
                child_profile = next(p for p in self.profiles if p["profile_id"] == child)
                for tag in child_profile['segment_tags']:
                    if tag not in segment_tags and tag['status'] == 'active':
                        _append_tag(tag)
            
            # Update parent profile segment tag attribute
            parent_profile['segment_tags'] = segment_tags

    async def _parent_profiles_df_numerical_type_conversion(self) -> None:
        ''' 
        Handle the conversion of numerical types 
        from float to their required type.
        NOTE: pandas converts numerical types to type float
        when column contains NaN values.
        '''
        # Set string representation of NaN in place of NaN floats
        self.parent_profiles_df = self.parent_profiles_df.replace(np.nan, 'NaN')

        # Known attribute numercial type conversions
        self.parent_profiles_df['rfm_score'] = self.parent_profiles_df['rfm_score'].replace('NaN', 0)
        self.parent_profiles_df['rfm_score'] = self.parent_profiles_df['rfm_score'].astype(int)

        existing_types_map = profilesIdenRepository.get_profile_key_types(account_id=self.account_id)

        def _change_dtype(column, value):
            for et in existing_types_map:
                if column.split('.')[1] == et['key']:
                    if et['type_'] == "<class 'int'>":
                        try:
                            return int(value)
                        except ValueError:
                            return value
                    elif et['type_'] == "<class 'float'>":
                        try:
                            return float(value)
                        except ValueError:
                            return value
                    else:
                        continue
            return value

        for column in self.parent_profiles_df.columns:
            if 'profile_data' in column or 'platform_info' in column:
                self.parent_profiles_df.loc[:, column] = \
                    self.parent_profiles_df[column].apply(lambda row: _change_dtype(column, row))

    def _parent_profiles_df_to_formatted_json(self) -> list:
        result = list()
        for _, row in self.parent_profiles_df.iterrows():
            parsed_row = {}
            for col_label, v in row.items():
                if v == 'NaN':
                    if 'profile_data' in col_label:
                        continue
                    if 'platform_info' in col_label:
                        continue
                keys = col_label.split('.')
                current = parsed_row
                for i, k in enumerate(keys):
                        if i==len(keys)-1:
                            current[k] = v
                        else:
                            if k not in current.keys():
                                current[k] = {}
                            current = current[k]
            try:
                parsed_row['profile_data']
            except KeyError:
                parsed_row['profile_data'] = {}
            
            try:
                parsed_row['platform_info']
            except KeyError:
                parsed_row['platform_info'] = {}

            result.append(parsed_row)
        return result

    async def _generate_profiles_batch(self) -> None:

        self.group_profiles_df['account_id'] = self.account_id
        self.group_profiles_df['authenticated_id_key'] = self.authenticated_id_key
        
        def apply_parent_customer_id(parent_profile : str) -> str:
            profile = next(p for p in self.profiles if p["profile_id"] == parent_profile)
            return profile['customer_id']

        def apply_authenticated_id_value(parent_profile : str) -> str:
            profile = next(p for p in self.profiles if p["profile_id"] == parent_profile)
            return profile['profile_data'][self.authenticated_id_key]
        
        def apply_merged_profiles(child_profiles : list) -> list:
            merged_profiles = list()
            for cp in child_profiles:
                profile = next(p for p in self.profiles if p["profile_id"] == cp)
                merged_profiles.append({
                    'profile_id' : profile['profile_id'],
                    'customer_id' : profile['customer_id']
                })
            return merged_profiles

        self.group_profiles_df['parent_customer_id'] = self.group_profiles_df.apply(lambda row: apply_parent_customer_id(row['parent_profile']), axis=1)
        self.group_profiles_df['authenticated_id_value'] = self.group_profiles_df.apply(lambda row: apply_authenticated_id_value(row['parent_profile']), axis=1)
        self.group_profiles_df['child_profiles'] = self.group_profiles_df.apply(lambda row: apply_merged_profiles(row['child_profiles']), axis=1)
        self.group_profiles_df.rename(columns={'child_profiles': 'merged_profiles', 'parent_profile' : 'parent_profile_id'}, inplace=True)
        self.profiles_batch = self.group_profiles_df.to_dict('records')


    async def _merge_profiles(self) -> None: 
        # get profiles
        await self._build_profiles_df()
        # group profiles on authenticated_id_key
        await self._group_profiles()
        # apply scores to each profile
        self.group_profiles_df['scores'] = self.group_profiles_df.apply(lambda row: self._apply_scores(row['profile_id']), axis=1)
        # specify parent profile
        self.group_profiles_df['parent_profile'] = self.group_profiles_df.apply(lambda row: self._select_parent_profile(row['scores']), axis=1)
        # specify child profiles
        self.group_profiles_df['child_profiles'] = self.group_profiles_df.apply(lambda row: self._specify_child_profiles(row['parent_profile'], row['profile_id']), axis=1)
        # Drop rows where number of child profiles < 1
        self.group_profiles_df['child_profile_count'] = self.group_profiles_df.apply(lambda row: self._specify_child_profile_count(row['parent_profile'], row['profile_id']), axis=1)
        
        # drop un needed rows + columns
        await self._drop_null_child_profiles()
        self.group_profiles_df = self.group_profiles_df.drop(columns=['profile_data.'+self.authenticated_id_key, 'scores', 'profile_id', 'child_profile_count'])
        
        # merge segment tags
        self.group_profile_dicts = self.group_profiles_df.to_dict(orient='records')
        await self._merge_segment_tags()

        # Shape profiles update
        self.parent_profiles_df = pd.json_normalize(self.profiles)

        # Get all child profile IDS for deletion.
        child_profiles = list(itertools.chain.from_iterable(self.group_profiles_df['child_profiles'].to_list()))
        # Drop all child profile rows from self.parent_profiles_df
        self.parent_profiles_df = self.parent_profiles_df[~self.parent_profiles_df['profile_id'].isin(child_profiles)]
        # Drop other unrequired columns not needed for profiles update request body
        self.parent_profiles_df = self.parent_profiles_df.drop(columns=['created_at', 'updated_at'])

        # Handle numerical type conversion for parent_profiles_df
        await self._parent_profiles_df_numerical_type_conversion()

        # Build and process AMQP messages to perform required profile merge actions.
        # Update parent profiles
        for profile in self._parent_profiles_df_to_formatted_json():
            await self._append_message_payload(message_body=profile, type_='profiles')

        
        for cp in child_profiles:
            # Delete child profiles
            await self._append_message_payload(message_body=cp, type_='profiles_delete')

            # Delete child profiles Rec cache
            await self._append_message_payload(message_body=cp, type_='rec_cache_delete')

        for gpd in self.group_profile_dicts:
            # segment profiles update
            await self._append_message_payload(message_body=gpd, type_='past_segment_profiles')

            # Event instance owenrship update
            await self._append_message_payload(message_body=gpd, type_='event_instance_profiles')

        # Publish messages
        # TODO : CHECK BACK HERE 
        await self._process_amqp_messages()

        # Create _create_merged_profiles_ref
        await self._generate_profiles_batch()
        await profilesIdenRepository.create_merged_profiles_ref(self.profiles_batch)

    # Entry point
    async def run(self) -> None: 
        try:
            self.b.track_compute_units('hours')
            await self._merge_profiles()
            await self._complete_job()
            self.b.complete_compute_units()
            self.logger.info('Completed Job!')
        except Exception as e:
            capture_exception(e)
            self.logger.critical(e)
            self.b.complete_compute_units()
            await self._dispose_job(ex=str(e))