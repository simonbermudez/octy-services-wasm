# module imports
from data.repositories.Iprofiles_repository import ProfilesInterface
from data.models.db_schemas import tbl_profiles, tbl_merged_profiles
from utils.utils import *
from api.routers.error_handlers import *
import data.context.db_context as ctx


# python imports
from typing import *
import json
from datetime import datetime as dt

# external imports
from pymongo.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps


class _ProfilesRepository(ProfilesInterface):
    """
        _ProfilesRepository
        Handles:
        - Retrieving profiles & merged profiles
        - Creating profiles
        - Updating profiles
        - Deleting profiles + events and segment tags
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    def get_profile_count(self, account_id : str) -> int:
        """
        A method used to return the count of all exisitng profiles associated with specififed account.

        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        
        return tbl_profiles.objects(account_id__exact=account_id).count()

    def get_profile_by_id(self, account_id : str, identifier : str) -> dict:
        """
        A method used to filter and return a list of profiles based the provided profile_id or customer_id.

        Parameters
        ----------
        account_id : str
            Octy account id
        identifier : str
            The profile_id or customer_id of the profile that should be returned.

        Returns
        ----------
        results : dict
        """
        profiles = tbl_profiles.objects((Q(profile_id__exact=identifier) & Q(account_id__exact=account_id)) \
            | (Q(customer_id__exact=identifier) & Q(account_id__exact=account_id)))

        if profiles:
            profile_dict = json.loads(profiles.to_json())
            profile_dict[0]['profile_id'] = profile_dict[0]['_id']
            profile_dict= _format_profile(profile_dict[0],tag_statuses=['active'])
            return profile_dict
        return None
    
    def get_profiles_by_identifiers(self, account_id : str, identifiers : list, tag_statuses : list, ids : bool = None, internal : bool = False) -> Union[list, list]:
        """
        A method used to filter and return a list of profiles based the provided profile_ids. multiple.

        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : str
            A list of identifiers (profile_ids | customer_ids)
        tag_statuses : list
            a list of statuses indicating which segment tags should be returned
        ids : bool
        internal : bool

        Returns
        ----------
        found_profiles : list
        not_found : list
        """
        found_profiles = []
        not_found = []
        if ids:
            profiles =  tbl_profiles._get_collection().find({
                    '$and' : [
                            {'$or' : [
                                {"_id" : { "$in" : identifiers}},
                                {"customer_id" : { "$in" : identifiers}}
                            ]},
                            {"account_id" : { "$eq" : account_id}}
                    ]
            },{"_id":1})
            for profile in profiles:
                profile['profile_id'] = profile['_id']
                _format_profile(profile, tag_statuses=tag_statuses, internal=internal)
                found_profiles.append(profile)

        else:
            profiles = tbl_profiles.objects((Q(profile_id__in=identifiers) & Q(account_id__exact=account_id)) \
                | (Q(customer_id__in=identifiers) & Q(account_id__exact=account_id)))

            for profile in profiles:
                profile_dict = json.loads(profile.to_json())
                profile_dict['profile_id'] = profile_dict['_id']
                profile_dict= _format_profile(profile_dict, tag_statuses=tag_statuses, internal=internal)
                found_profiles.append(profile_dict)
        
        # get all not found ids
        for p in identifiers:
            exists=next((key for key in found_profiles if key['profile_id'] == p), None)
            if not exists:
                not_found.append(p)
        
        return found_profiles, not_found

    def get_profiles_by_params(self,
                    account_id : str,
                    cursor : int = None, 
                    segments : list = None, 
                    rfm_values : list = None, 
                    churn_prob : str = None) -> Union[list, int]:
        """
        A method used to filter and return a list of profiles based on the 
        provided parameters.

        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int
            Pagination cursor
        segments : list
            List of segment identifiers
        rfm_values : list
            two integers in a list representing the upper and lower bounds 
            of the desired FRM range to filter profiles by
        churn_prob : str
            label representing the desired churn probability to filter profiles by

        Returns
        ----------
        profiles : list
        total : int
        """

        query_and = [{
            "account_id" : { "$eq" : account_id}
        }]

        if rfm_values != None:
            query_and.append(
            {
                "rfm_score" : {
                "$gt" : rfm_values[0], "$lt" : rfm_values[1]
            }
            })

        if churn_prob != None:
            query_and.append(
            {
                "churn_probability" : { "$eq" : churn_prob}
            })

        if segments != None:
            if len(segments) == 1:
                query_and.append(
                {
                    "segment_tags.segment_tag" :{ 
                    "$eq" : segments[0]
                }
                })
            else:
                seg_queries = []
                for segment in segments:
                    seg_queries.append(
                        {
                            '$and' : [
                                {'segment_tags.segment_tag' : segment.strip()},
                                {'segment_tags.status' : 'active'}
                            ]
                        
                        }
                    )
                query_and.append(
                {
                    "$or" :seg_queries
                })
        
        results_cursor = tbl_profiles._get_collection().find({'$and' : query_and}).skip(cursor).limit(100)
        total = tbl_profiles._get_collection().find({'$and' : query_and}).count()
        raw_res = json.loads(dumps(list(results_cursor), indent = 2))

        #format profiles
        for profile in raw_res:
            profile['profile_id'] = profile['_id']
            _format_profile(profile, tag_statuses=['active'])

        return raw_res, total

    def get_all_profiles(self, account_id : str, tag_statuses : list, cursor : int = None, ids : bool = None, status : str = 'active', limit : int = 100, internal : bool = False) -> Union[list, int]:
        """
        A method used to return all profiles associated with specified account

        Parameters
        ----------
        account_id : str
            Octy account id
        tag_statuses : list
            a list of statuses indicating which segment tags should be returned
        cursor : int
            pagination cursor
        ids : bool
            Only return profile ids
        status : str
        internal : bool

        Returns
        ----------
        profiles/ profiles ids : list 
        total : int
        or
        results : list, int
        """
        found_profiles = []
        if ids:
            profile_ids =  tbl_profiles._get_collection().find({
                "account_id" : { "$eq" : account_id},
                "status" : { "$eq" : status}
            },{"_id":1}).skip(cursor).limit(limit)
            #format profiles
            for profile in profile_ids:
                #profile_dict = dumps(list(profile_ids), indent = 2)
                profile['profile_id'] = profile['_id']
                _format_profile(profile, tag_statuses=tag_statuses, internal = internal)
                found_profiles.append(profile)
        else:
            profiles = tbl_profiles.objects(account_id__exact=account_id, status__exact=status).skip(cursor).limit(limit)
            #format profiles
            for profile in profiles:
                profile_dict = json.loads(profile.to_json())
                profile_dict['profile_id'] = profile_dict['_id']
                _format_profile(profile_dict, tag_statuses=tag_statuses,  internal = internal)
                found_profiles.append(profile_dict)

        total = tbl_profiles.objects(account_id__exact=account_id, status__exact=status).count()    
        return found_profiles, total

    def get_merged_profiles(self, account_id : str, identifiers : list) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : list
            A list of identifiers (profile_ids | customer_ids)

        Returns
        ----------
        merged_profiles : list
        """
        merged_profiles = list()
        queries_idxs = list()
        queries = [{
            '$facet' : {

            }
        }]

        for idx, i in enumerate(identifiers): 
            queries[0]['$facet']['query'+str(idx)] = [
                {'$match' : 
                    { 
                        '$and' : [
                            {"account_id" : { "$eq" : account_id}},
                            {'$or' : [
                                    {"merged_profiles.profile_id" : { "$eq" : i}},
                                    {"merged_profiles.customer_id" : { "$eq" : i}},
                                    {"parent_profile_id" : { "$eq" : i}},
                                    {"parent_customer_id" : { "$eq" : i}}
                            ]}
                        ] 
                    }
                },
                { '$sort' : { 'created_at' : -1 } },
                { '$limit' : 1 }
            ]
            queries_idxs.append('query'+str(idx))

        results = tbl_merged_profiles._get_collection().aggregate(queries)
        try:
            results_dicts = json.loads(dumps(results))[0]
        except KeyError:
            return merged_profiles

        for q in queries_idxs:
            try:
                merged_profiles.append(
                    {
                        'merged_profiles' : results_dicts[q][0]['merged_profiles'],
                        'parent_profile_id' : results_dicts[q][0]['parent_profile_id'],
                        'parent_customer_id' : results_dicts[q][0]['parent_customer_id'], 
                        'authenticated_id_key' : results_dicts[q][0]['authenticated_id_key'], 
                        'authenticated_id_value' : results_dicts[q][0]['authenticated_id_value'],
                        'merged_at' : int_to_dt(results_dicts[q][0]['created_at']['$date'] , as_str=True)
                    }
                )
            except IndexError:
                continue

        return merged_profiles
        
    def create_profiles(self, profiles_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles_batch : List
            list of profile object dictonaries (valid profile objects)

        Returns
        ----------
        created_profiles, failed_to_create profiles
        """
        profile_instances = []
        customer_ids = []
        for profile in profiles_batch:
            profile_instances.append(
                tbl_profiles(
                    profile_id=profile['profile_id'],
                    customer_id=profile['customer_id'],
                    account_id=profile['account_id'],
                    profile_data=profile['profile_data'],
                    platform_info=profile['platform_info'],
                    has_charged=profile['has_charged']
                )
            )
            customer_ids.append(profile['customer_id'])

        #BULK WRITE OPERATION
        invalid=[]
        bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        for profile in profile_instances:
            bulk_operation.insert(profile.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                invalid.append(err['op'].to_dict()['customer_id'])

        valid = list(set(customer_ids) - set(invalid))

        failed_to_create=[]
        for in_ in invalid:
            failed_to_create.append(
                {
                    'customer_id': in_,
                    'error_message' : f'Another profile exists with provided customer_id : {in_}'
                }
            )
        created_profiles=[]
        for v in valid:
            profile=next((d for i,d in enumerate(profiles_batch) if v == d['customer_id']),None)
            if profile:
                profile.pop('account_id', None)
                created_profiles.append(profile)
        
        return created_profiles, failed_to_create

    async def update_profiles(self, profiles_batch : list, internal : bool) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles_batch : list
            list of profile object dictonaries (valid profile objects)
        internal : bool
            Did update request come from an internal process. Do not
            allow client to update certain profile attributes

        Returns
        ----------
        updated profiles : list
        not found / invalid profiles: list
        """

        updated_profiles = []
        failed_to_update=[]
        not_existing_profiles = []
        profile_ids = [] # provided profile ids array

        # determine valid profiles
        for profile in profiles_batch:
            if profile['profile_id'] in profile_ids:
                raise OctyException(400,'An error occurred when validating request.', [{'error_message' : f'Identical profile identifers supplied. Found duplicate profile_id : {profile["profile_id"]}', 
                'extended_help': Config['PROFILES_EXTENDED_HELP']}])
            profile_ids.append(profile['profile_id'])

        profiles = json.loads(tbl_profiles.objects(profile_id__in=profile_ids, account_id__exact=profiles_batch[0]['account_id']).to_json())
        if not profiles:
            for profile in profiles_batch:
                failed_to_update.append(
                    {
                        'profile_id' : profile['profile_id'],
                        'error_message' : f'No profile found with profile_id : {profile["profile_id"]}'
                    }
                )
            return updated_profiles, failed_to_update
     
        for p in profile_ids:
            exists=next((key for key in profiles if key['_id'] == p), None)
            if not exists:
                customer=next((key for key in profiles_batch if key['profile_id'] == p), None)
                not_existing_profiles.append(customer['customer_id'])
                failed_to_update.append(
                    {
                        'profile_id': p,
                        'error_message' : f'No profile exists with provided profile_id : {p}'
                    }
                )

        #BULK UPDATE OPERATION
        bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        for p in profiles:
            profiles_batch_obj = next(key for key in profiles_batch if key['profile_id'] == p['_id'])

            # build update dict
            set_dict = DictConditional(lambda x: x != None)
            set_dict['_id'] = profiles_batch_obj['profile_id']
            set_dict['customer_id'] = profiles_batch_obj['customer_id'] if profiles_batch_obj['customer_id'] != None else p['customer_id']
            set_dict['profile_data'] = profiles_batch_obj['profile_data'] if profiles_batch_obj['profile_data'] != None else p['profile_data']
            set_dict['platform_info'] = profiles_batch_obj['platform_info'] if profiles_batch_obj['platform_info'] != None else p['platform_info']
            set_dict['has_charged'] = profiles_batch_obj['has_charged'] if profiles_batch_obj['has_charged'] != None else p['has_charged']
            set_dict['status'] = profiles_batch_obj['status'] if profiles_batch_obj['status'] != None else p['status']
            set_dict['updated_at'] = dt.now()
            if internal:
                set_dict['rfm_score'] = profiles_batch_obj['rfm_score'] if profiles_batch_obj['rfm_score'] != None else p['rfm_score']
                set_dict['rfm_segment_desc'] = profiles_batch_obj['rfm_segment_desc'] if profiles_batch_obj['rfm_segment_desc'] != None else p['rfm_segment_desc']
                set_dict['churn_probability'] = profiles_batch_obj['churn_probability'] if profiles_batch_obj['churn_probability'] != None else p['churn_probability']
                set_dict['ltv_prediction'] = profiles_batch_obj['ltv_prediction'] if profiles_batch_obj['ltv_prediction'] != None else p['ltv_prediction']
                set_dict['current_ltv'] = profiles_batch_obj['current_ltv'] if profiles_batch_obj['current_ltv'] != None else p['current_ltv']
                set_dict['segment_tags'] = _format_segment_tags(profiles_batch_obj['segment_tags'], p['segment_tags']) if profiles_batch_obj['segment_tags'] != None else _format_segment_tags(p['segment_tags'], p['segment_tags'], tags_updated=False)

            bulk_operation.find({
                '$and' : [
                    {"_id" : { "$eq" : p['_id']}},
                    {"account_id" : { "$eq" : p['account_id']}}
                ]
            }).update(
                {
                    "$set" : set_dict
                }
            )

            # append updated profile to return array
            profiles_batch_obj['created_at'] = p['created_at']
            profiles_batch_obj['updated_at'] = dt.now()
            updated_profiles.append(_format_profile(profiles_batch_obj, tag_statuses=['active'], internal=internal))
        
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                if err['code'] == 11000:
                    mes = f"Another profile exists with provided customer_id : {err['op']['u']['$set']['customer_id']}"
                else:
                    mes = f"Unknown error occurred when updating profile with customer_id : {err['op']['u']['$set']['customer_id']}"
                failed_to_update.append({
                        'profile_id' : err['op']['u']['$set']['profile_id'],
                        'customer_id':  err['op']['u']['$set']['customer_id'],
                        'error_message' : mes
                    })


                updated_profiles = list(filter(lambda i : i['profile_id'] != err['op']['u']['$set']['profile_id'], updated_profiles))


        return updated_profiles, failed_to_update

    async def delete_profiles(self, profiles_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles_batch : list
            list of profile object dictonaries to delete

        Returns
        ----------
        deleted_profiles : list
        failed_to_delete : list
        """
        deleted_profiles=[]
        failed_to_delete=[]
        profile_ids=[]

        for profile in profiles_batch:
            profile_ids.append(profile['profile_id'])


        profiles = json.loads(tbl_profiles.objects(profile_id__in=profile_ids, account_id__exact=profiles_batch[0]['account_id']).to_json())
        if not profiles:
            for profile in profiles_batch:
                failed_to_delete.append(
                    {
                        'profile_id' : profile['profile_id'],
                        'error_message' : f'No profile found with profile_id : {profile["profile_id"]}'
                    }
                )
            return deleted_profiles, failed_to_delete

        
        
        bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        for profile in profiles_batch:
            p_object=next((key for key in profiles if key['_id'] == profile['profile_id'] and key['account_id'] == profile['account_id']), None)
            if p_object:
                deleted_profiles.append(
                    {
                        'profile_id': p_object['_id'],
                        'customer_id': p_object['customer_id']
                    }
                )
            else:
                failed_to_delete.append(
                    {
                        'profile_id' : profile['profile_id'],
                        'error_message' : f'No profile found with profile_id : {profile["profile_id"]}'
                    }
                )

            bulk_operation.find({
                '$and' : [
                    {  "_id" : { "$eq" : profile['profile_id'] }  },
                    {  "account_id" : { "$eq" : profile['account_id'] }  }
                ]
            }).remove()

        bulk_operation.execute()

        return deleted_profiles, failed_to_delete

    async def update_delete_segment_tags(self, account_id : str, segment_ids : list, action : str) -> None: 
        """
        Either update the status of tags to 'pending_deletion' or 
        delete all segment tags in provided list. This is used when segment definitions are deleted.

        Parameters
        ----------
        account_id : str
            octy account id
        segment_ids : list
        action : str
            update or delete

        Returns
        ----------
        None
        """

        if action == 'update':
            #BULK UPDATE OPERATION
            bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()

            for seg in segment_ids:
                # find all segment tags in profiles and update status to 'pending deletion'

                bulk_operation.find({
                    '$and' : [
                        {"account_id" : { "$eq" : account_id} },
                        {"segment_tags.segment_id" : { "$eq" : seg.segment_id} }
                    ]
                }).update(
                    {
                        "$set" : 
                            {   
                                "segment_tags.$.status":"pending_deletion",
                                "segment_tags.$.updated_at":dt.now()
                            }
                    }
                )
            bulk_operation.execute()

        elif action == 'delete':
            for seg in segment_ids:
                tbl_profiles.objects(account_id__exact=account_id, segment_tags__segment_id__exact=seg.segment_id).update(pull__segment_tags__segment_id=seg.segment_id)
            
    # Single segment tag operations.
    async def create_segment_tags(self, account_id : str, profile_id : str, segment_tags : list) -> None:
        """

        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to create

        Returns
        ----------
        None
        """
        #TODO: add safeguard here to ensure duplicate tags are not added to a profile
        bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        for seg in segment_tags:
    
            bulk_operation.find({
                '$and' : [
                    {"account_id" : { "$eq" : account_id} },
                    {"_id" : { "$eq" : profile_id} }
                ]
            }).update(
                {
                    "$push" : 
                        {   
                            "segment_tags": {
                                "segment_id" : seg['segment_id'],
                                "segment_tag" : seg['segment_tag'],
                                "status" : seg['status'],
                                "created_at" : dt.now()
                            }
                        }
                }
            )
        bulk_operation.execute()

    async def update_segment_tags(self, account_id : str, profile_id : str, segment_tags : list) -> None:
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to update

        Returns
        ----------
        None
        """
        # bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        # for seg in segment_tags:
        #     # find all segment tags in profiles and update status to 'pending deletion'

        #     bulk_operation.find({
        #         '$and' : [
        #             {"account_id" : { "$eq" : account_id} },
        #             {"_id" : { "$eq" : profile_id} },
        #             {"segment_tags.segment_id" : { "$eq" : seg['segment_id']} },
        #             {
        #                 '$or' : [
        #                     {"segment_tags.status" : { "$eq" : "active"}},
        #                     {"segment_tags.status" : { "$eq" : "pending"}}
        #                 ]
        #             }
        #         ]
        #     }).update(
        #         {
        #             "$set" : 
        #                 {   
        #                     "segment_tags.$.status":seg['status'],
        #                     "segment_tags.$.updated_at":dt.now()
        #                 }
        #         }
        #     )
        # bulk_operation.execute()
        profile = tbl_profiles.objects( ( Q(profile_id__exact=profile_id) & Q(account_id__exact=account_id) )).first()
        if profile:
            for segment_tag in segment_tags:
                for tag in profile.segment_tags:
                    if tag.segment_id != segment_tag['segment_id']:
                        continue
                    if tag.status == 'pending_deletion' or tag.status == 'inactive':
                        continue
                    tag.status = segment_tag['status']
            profile.save()

    async def delete_segment_tags(self, account_id : str, profile_id : str, segment_tags : list) -> None:
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_id : str
            Octy profile identifier
        segment_tags : list
            List of segment tags to delete

        Returns
        ----------
        None
        """

        bulk_operation = tbl_profiles._get_collection().initialize_unordered_bulk_op()
        for seg in segment_tags:

            bulk_operation.find({
                '$and' : [
                    {"account_id" : { "$eq" : account_id} },
                    {"_id" : { "$eq" : profile_id} }
                ]
            }).update(
                {
                    "$pull" : {   
                        "segment_tags" : { 
                            "segment_id" : seg['segment_id']
                        } 
                    }
                }
            )
        bulk_operation.execute()
        #, "status" :"pending_deletion"
            
    def set_profile_key_type(self, account_id : str, profile_key_type : dict) -> None:
        """
        Parameters
        ----------
        account_id : str
            octy account id
        profile_key_type : dict
            ex : {'key' : 'age', 'type_' : '<class 'int'>'}

        Returns
        ----------
        None
        """
        ctx.redis_conn.sadd(f'{account_id}_profile_key_types', json.dumps(profile_key_type))

    def get_profile_key_types(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            octy account id

        Returns
        ----------
        list
        """
        profile_key_types = json.loads(json.dumps([json.loads(s) for s in 
            list(ctx.redis_conn.smembers(f'{account_id}_profile_key_types'))]))
        return profile_key_types
    
def _format_profile(profile : dict, tag_statuses : list, internal : bool = False) -> dict:
    '''
        Format profile return objects
    '''
    
    profile.pop('_id', None)
    profile.pop('account_id', None)
    profile.pop('ltv_prediction', None)
    profile.pop('current_ltv', None)

    # Format segment tags 
    if internal:
        try:
            # Filter segment tags, based on status, returned with each profile.
            # DO NOT remove tag attributes
            valid_tags = []
            for tag in profile['segment_tags']:
                if tag['status'] not in tag_statuses:
                    continue
                valid_tags.append(tag)
            profile['segment_tags'] = valid_tags
        except Exception: pass

    else:
        try:
            # Filter segment tags, based on status, returned with each profile.
            # Remove un-needed tag attributes. 
            valid_tags = []
            for tag in profile['segment_tags']:
                if tag['status'] not in tag_statuses:
                    continue
                tag.pop('segment_id', None)
                tag.pop('status', None)
                tag.pop('updated_at', None)
                tag['created_at'] = int_to_dt(tag['created_at']['$date'], as_str=True)
                valid_tags.append(tag)
            profile['segment_tags'] = valid_tags
        except Exception: pass

    try:
        profile['created_at'] = int_to_dt(profile['created_at']['$date'], as_str=True) if profile['created_at'] != None else None
        try:
            profile['updated_at'] = int_to_dt(profile['updated_at']['$date'], as_str=True) if profile['updated_at'] != None else None
        except TypeError:
            profile['updated_at'] = profile['updated_at'].strftime('%a, %d %b %Y %H:%M:%S GMT')
    except KeyError:
        pass

    return profile

def _format_segment_tags(profile_segment_tags , current_segment_tags, tags_updated=True) -> list:
    '''
        Updating profile segment tags
    '''
    # format segment tags
    if profile_segment_tags != None:
        if current_segment_tags ==None:
            current_segment_tags = []
        
        for tag in profile_segment_tags:
            try:
                exists=next((key for key in current_segment_tags if key['segment_id'] == tag.segment_id), None)
            except AttributeError:
                exists=next((key for key in current_segment_tags if key['segment_id'] == tag['segment_id']), None)
            if not exists:
                try:
                    current_segment_tags.append(
                        {
                            'segment_id' : tag.segment_id, 
                            'segment_tag' : tag.segment_tag,
                            'status' : tag.status if tag.status else 'active',
                            'created_at' : dt.now(),
                            'updated_at' : None
                        }
                    )
                except AttributeError:
                    current_segment_tags.append(
                        {
                            'segment_id' : tag['segment_id'], 
                            'segment_tag' : tag['segment_tag'],
                            'status' : tag['status'] if tag['status'] else 'active',
                            'created_at' : dt.now(),
                            'updated_at' : None
                        }
                    )

            else:
                if tags_updated:
                    exists['updated_at'] = dt.now()
                try:
                    exists['created_at'] = int_to_dt(exists['created_at']['$date'], as_str=False)
                except TypeError:
                    pass
                try:
                    exists['status'] = tag.status
                except AttributeError:
                    exists['status'] = tag['status']

    else:
        return []
    
    return current_segment_tags



profilesRepository = _ProfilesRepository()