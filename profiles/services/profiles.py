# module imports
from data.repositories.implementation.profiles_repository import profilesRepository
from api.routers.request_models.profiles import *
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


class ProfilesService():
    """
        ProfilesService
        Handles:
        - Get Profiles
        - Profiles creation
        - Update profiles
        - Delete profiles
        ...

        Attributes
        ----------
        account : Octy account
        account_id : str
    """
    def __init__(self, account : Account, account_id : str = None): 
        self.account = account
        self.account_id = account_id if account_id != None else account.account_id

    def get_profiles(self, 
                    segments : list, 
                    rfm_values : list, 
                    churn_prob : str, 
                    identifiers : list = None, 
                    cursor : int = None) -> Union[dict, int]: 
        """
        A method used to filter and return a list of profiles based on the 
        provided parameters.

        Parameters
        ----------
        segments : list
            List of segment identifiers
        rfm_values : list
            two integers in a list representing the upper and lower bounds 
            of the desired FRM range to filter profiles by
        churn_prob : str
            label representing the desired churn probability to filter profiles by
        identifiers : list
            list of profile_id(s) or customer_id(s)
        cursor : int
            Pagination cursor

        Returns
        ----------
        profiles : dict
        total : int
        """

        if identifiers != None and cursor == 0:
            
            profiles, _ = profilesRepository.get_profiles_by_identifiers(account_id=self.account_id, 
                identifiers=identifiers, 
                tag_statuses=['active'], 
                ids=False, 
                internal=False)
            count = len(profiles)
            if count<1:
                raise OctyException(400, 'Invalid customer identifier(s) provided', 
                [{'error_message' : 'No customer profiles were found with the provided identifier(s)', 
                'extended_help': Config['PROFILES_EXTENDED_HELP']}])

            return profiles, count
            
        elif identifiers == None and cursor != None:
            
            profiles,total = profilesRepository.get_profiles_by_params(account_id=self.account.account_id, 
                                                cursor=cursor,
                                                segments=segments,
                                                rfm_values=rfm_values,
                                                churn_prob=churn_prob)
            if len(profiles)<1:
                raise OctyException(400, 'No customer profiles found', 
                [{'error_message' : 'No customer profiles found with the provided query parameters or pagination cursor exhausted', 
                'extended_help': Config['PROFILES_EXTENDED_HELP']}])
            return profiles, total

    def get_profiles_meta(self, identifiers : list = None) -> list:
        """
        A method used to return a list of merged profiles.

        Parameters
        ----------
        identifiers : list
            A list of identifiers (profile_ids | customer_ids)

        Returns
        ----------
        identifiers_meta : list
        """
   
        merged_profiles = profilesRepository\
            .get_merged_profiles(account_id=self.account.account_id, identifiers=identifiers)
        found_profiles, _ = profilesRepository.get_profiles_by_identifiers(account_id=self.account_id, 
                identifiers=identifiers, 
                tag_statuses=['active'], 
                ids=False, 
                internal=True)

        def _val_or_none(obj, key):
            try:
                return obj[key]
            except TypeError:
                return None

        identifiers_meta = list()
        for i in identifiers: 
            exists=next((key for key in found_profiles if key['profile_id'] == i or key['customer_id'] == i), None)
            parent_merged=next((key for key in merged_profiles if key['parent_profile_id'] == i or key['parent_customer_id'] == i), None)
            try:
                child_merged=None
                for mp in merged_profiles: 
                    child_merged=next((key for key in mp['merged_profiles'] if key['profile_id'] == i or key['customer_id'] == i), None)
                    if child_merged:
                        child_merged=mp
                        break
            except Exception as e: 
                print(e)
                child_merged = None

            is_parent = False
            if parent_merged:
                is_parent = True
            
            identifiers_meta.append(
            {
                "provided_identifier" : i,
                "profile" : {
                    "profile_exists" : True if exists != None else False,
                    "profile_id" : None if _val_or_none(exists, 'profile_id') is None else exists['profile_id'],
                    "customer_id" : None if _val_or_none(exists, 'customer_id') is None else exists['customer_id'],
                    "created_at" : None if _val_or_none(exists, 'created_at') is None else exists['created_at'],
                    "updated_at" : None if _val_or_none(exists, 'updated_at') is None else exists['updated_at']
                },
                "merged_info" : {
                        "was_merged" : False if not is_parent and not child_merged else True,
                        "merged_at" : _val_or_none(parent_merged, 'merged_at') if is_parent else _val_or_none(child_merged, 'merged_at'),
                        "authenticated_id_key" : _val_or_none(parent_merged, 'authenticated_id_key') if is_parent else _val_or_none(child_merged, 'authenticated_id_key'),
                        "authenticated_id_value" : _val_or_none(parent_merged, 'authenticated_id_value') if is_parent else _val_or_none(child_merged, 'authenticated_id_value'),
                        "parent_or_child" : "parent_profile" if is_parent else ("child_profile" if child_merged else None),
                        "parent_profile" : {
                                "parent_profile_id" : None if _val_or_none(child_merged, 'parent_profile_id') is None else child_merged['parent_profile_id'],
                                "parent_customer_id" : None if _val_or_none(child_merged, 'parent_customer_id') is None else child_merged['parent_customer_id'],
                        },
                        "merged_child_profiles" : parent_merged['merged_profiles'] if _val_or_none(parent_merged, 'merged_profiles') else (child_merged['merged_profiles'] if _val_or_none(child_merged, 'merged_profiles') else []),

                }
            })
        return identifiers_meta

    def create_profiles(self, profiles : CreateProfiles) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles : CreateProfiles
            CreateProfiles request model instance

        Returns
        ----------
        Created and failed to create profiles : Union[list, list]
        """

        # assess allowed limits
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              profilesRepository.get_profile_count(self.account.account_id),
                              len(profiles.profiles))
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'error_message' : f'This request could not be completed as the number of profiles sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} profiles.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        profiles_batch = []
        for profile in profiles.profiles:
            profiles_batch.append(
                {
                    'profile_id' : generate_uid('profile'),
                    'customer_id' : profile.customer_id,
                    'account_id' : self.account.account_id,
                    'profile_data' : profile.profile_data,
                    'platform_info' : profile.platform_info,
                    'has_charged' : profile.has_charged
                }
            )
        
        #validate client provided keys
        res, error = self._validate_profile_key_types(profiles_batch)
        if not res:
            raise OctyException(400,'An error occurred when validating keys.', [{'error_message' : error, 
                'extended_help': Config['PROFILES_EXTENDED_HELP']}])

        created, failed = profilesRepository.create_profiles(profiles_batch)

        if len(created) < 1:
            raise OctyException(400, 'No profiles created!', failed)

        return created, failed

    async def update_profiles(self, profiles : UpdateProfiles, internal : bool) -> Union[list, list]:
        """
        Method can be called from API client to update basic profiles data.
        Or can be called from AMQP [internal] to update:
         - segment tags
         - churn preidction
         - rfm score + rfm desc

        Parameters
        ----------
        profiles : UpdateProfiles
            UpdateProfiles request model instance
        internal : bool
            Was update initated by client or an internal process [AMQP or Client HTTP]

        Returns
        ----------
        Updated and failed to update profiles : Union[list, list]
        """
        profiles_batch = []
        for profile in profiles.profiles:
            profiles_batch.append(
                {
                    'profile_id' : profile.profile_id,
                    'customer_id' : profile.customer_id,
                    'account_id' : self.account_id,
                    'profile_data' : profile.profile_data,
                    'platform_info' : profile.platform_info,
                    'has_charged' : profile.has_charged,
                    'status' : profile.status,
                    'rfm_score' : profile.rfm_score if profile.rfm_score != None else None,
                    'rfm_segment_desc' : profile.rfm_segment_desc if profile.rfm_segment_desc != None else None,
                    'churn_probability' : profile.churn_probability if profile.churn_probability != None else None,
                    'ltv_prediction' : profile.ltv_prediction if profile.ltv_prediction != None else None,
                    'current_ltv' : profile.current_ltv if profile.current_ltv != None else None,
                    'segment_tags' : profile.segment_tags if profile.segment_tags != None else None,

                }
            )

        if not internal:
            #validate client provided keys
            res, error = self._validate_profile_key_types(profiles_batch)
            if not res:
                raise OctyException(400,'An error occurred when validating keys.', [{'error_message' : error, 
                    'extended_help': Config['PROFILES_EXTENDED_HELP']}])

        updated, failed = await profilesRepository.update_profiles(profiles_batch, internal=internal)

        if len(updated) < 1:
            raise OctyException(400, 'No profiles updated!', failed)

        return updated, failed

    async def delete_profiles(self, profiles : DeleteProfiles, identification_job : bool = False, loop : Any = None) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles : DeleteProfiles
            DeleteProfiles request model instance
        identification_job : bool
            was this method called as the result of a profile identification job?
    
        Returns
        ----------
        Deleted and failed to delete profile ids : Union[list, list]
        """
        profiles_batch=[]
        for p in profiles.profiles:
            profiles_batch.append({
                "profile_id" : p,
                "account_id" : self.account_id
            })
            if not identification_job:
                loop.create_task(amqpPublisher.send_message(routing_key='events.cmd.delete',
                    payload={
                        'account_id' : self.account.account_id,
                        'profile_id' : p
                    }))

        deleted , failed = await profilesRepository.delete_profiles(profiles_batch)

        if len(deleted) < 1:
            raise OctyException(400, 'No profiles deleted!', failed)
        return deleted, failed

    def _validate_profile_key_types(self,new_customer_profiles : dict) -> Union[bool, str]:
        '''
        To ensure training data created from customer profiles is not corrupted, 
        the values in each key value pair across all customer profiles profile_data & platform_info in an account must be valid json and of the same data type. 
        For example, if the key 'os' exists in any other customer profile and is of data type 'string', the values for all future 'os' keys must be of type 'string'
        Returns result (bool), error message (string)
        '''
        try:
            
            def is_json(myjson):
                try:
                    if type(myjson) != str:
                        myjson=json.dumps(myjson)
                    json_object = json.loads(myjson)
                except ValueError as e:
                    return False
                return True
            
            def profile_json_to_dict(platform_info, profile_data, customer_id):
                '''
                merge platform_info and profile_data and return as single dict
                '''
                if type(platform_info) != str:
                    platform_info=json.dumps(platform_info)

                if type(profile_data) != str:
                    profile_data=json.dumps(profile_data)

                #prevent duplicate keys across platform_info and profile_data
                profile_keys=[]
                for k,_ in json.loads(platform_info).items():
                    if k not in profile_keys:
                        profile_keys.append(k)
                    else:
                        return False, 'Duplicate key: \'{k}\', provided in profile with customer_id: {p}'.format(k=k, p=customer_id),{}
                for k,_ in json.loads(profile_data).items():
                    if k not in profile_keys:
                        profile_keys.append(k)
                    else:
                        return False, 'Duplicate key: \'{k}\', provided in profile with customer_id: {p}'.format(k=k, p=customer_id),{}

                return_dict={}
                return_dict.update(json.loads(platform_info))
                return_dict.update(json.loads(profile_data))
                return True, '', return_dict

            def build_map(profiles, new_existing):
                '''
                Build key <-> type map_ for provided customer profiles (list[dicts])
                Return result (bool), error (string), populated map_
                '''
                map_=[]
                for profile in profiles:

                    # If profile data or platform info empty return error. <- must have at least one key in each.
                    if profile['profile_data'] == '{}' or profile['platform_info'] == '{}' :
                        return False, 'Both profile_data and platform_info must contain at least one single key pair. eg: {\'age\' : \'30\'}', []
                    
                    if '[]' in profile['profile_data'] or '[]' in profile['platform_info'] :
                        return False, 'Both profile_data and platform_info must contain at least one single key pair. eg: {\'age\' : \'30\'}', []

                    if not is_json(profile['platform_info']) or not is_json(profile['profile_data']):
                        return False, 'Error occurred when attempting to create customer profiles, \
                        invalid json structure provided for either profile_data or platform_info.', []

                    res, error, profile_data_info_dict = profile_json_to_dict(profile['platform_info'], profile['profile_data'], profile['customer_id'])
                    if not res:
                        return False, error, []
                    for k,v in profile_data_info_dict.items():
                        # if key does not exist in new_keys, append with type.
                        x=next((d for i,d in enumerate(map_) if k == d['key']),None)
                        if not x:
                            map_.append(
                                    {
                                        'key' : k,
                                        'type_': str(type(v))
                                    }
                                )
                        else:
                            #if key exists, check type
                            if str(type(v)) != x['type_']:
                                return False, f"Invalid type provided for key \'{k}\'. Got type {x['type_']} expected type {type(v)} according the data type of the value for the first instance of the key: {k}.", [] #provided in profile with customer_id: {profile["customer_id"]}

                return True, '', map_
                               
            existing_types_map = profilesRepository.get_profile_key_types(account_id=self.account_id)

            #build map for new profiles
            res, error, new_types_map = build_map(new_customer_profiles, 'new')
            if not res:
                return False, error
            
            if len(existing_types_map) > 0 :
                #compare types for each key in both maps, existing_types_map being classed as the truth.
                for k_v_pair in new_types_map:
                    #check if k_v_pair['key'] exists in existing_types_map, if not pass (this will become the truth value for this new key)
                    existing_type = next((et for et in existing_types_map if et['key'] == k_v_pair['key']), None)
                    if existing_type != None:
                        #if it does exist, compare types.
                        if existing_type['type_'] != k_v_pair['type_']:
                            return False, f"Invalid type provided for key \'{k_v_pair['key']}\'. Got type {k_v_pair['type_']} expected type {existing_type['type_']}"
                    else:
                        profilesRepository.set_profile_key_type(account_id=self.account_id, profile_key_type=k_v_pair)
            else:
                # create new profile key types for each
                for k_v_pair in new_types_map:
                    profilesRepository.set_profile_key_type(account_id=self.account_id, profile_key_type=k_v_pair)

            return True, ''

        except Exception as err:
            print(err)
            return False, 'Unknown error occurred. Typically, this is caused by malformed profile_data or platform_info. Please ensure you provided a valid JSON key pair object within both profile_data and platform_info for each new profile.'

    #INTERNAL

    async def grouped_segmentation_database_operations(self, operations : list) -> None:
        """
        This method allows the segmentaion worker to group Database oprations, 
        that need to be performed synchronously, in a single AMQP message. Each operation
        will be perfomed in the specified order.
        Parameters
        ----------
        operations : list
            List of operations segmentation
    
        Returns
        ----------
        None
        """
        for op in operations: 
            #switch through action 
            try:
                if op['action'] == 'create':
                    await profilesRepository.create_segment_tags(self.account_id, op['operation_payload']['profile_id'], op['operation_payload']['segment_tags'])
                elif op['action'] == 'update':
                    await profilesRepository.update_segment_tags(self.account_id, op['operation_payload']['profile_id'], op['operation_payload']['segment_tags'])
                elif op['action'] == 'delete':
                    await profilesRepository.delete_segment_tags(self.account_id, op['operation_payload']['profile_id'], op['operation_payload']['segment_tags'])
            except KeyError:
                continue

    def get_profiles_internal(self, profiles : GetProfilesInternal, status : str, cursor : int, ids : bool) -> Union[list, list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int
            Pagination cursor
        ids : bool
            Only get profiles id(s)
        Returns
        ----------
        found profiles : list
        not found profiles : list
        total : int
        """
        not_found = None

        if profiles.get_all:

            profiles, total = profilesRepository.get_all_profiles(account_id=self.account_id,
                tag_statuses=profiles.tag_statuses, 
                cursor=cursor, 
                ids=ids,
                status=status, 
                limit=2000, 
                internal=True)

        else:

            profiles, not_found = profilesRepository.get_profiles_by_identifiers(account_id=self.account_id, 
                identifiers=profiles.profiles, 
                tag_statuses=profiles.tag_statuses, 
                ids=ids, 
                internal=True)

            total = len(profiles)

        if len(profiles)<1:
            raise OctyException(400, 'No profiles found', 
            [{'error_message' : 'No profiles found or pagination cursor exhausted', 
            'extended_help': ''}])
        return profiles, not_found ,total
