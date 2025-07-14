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
from pymongo import InsertOne, UpdateOne, DeleteOne, UpdateMany

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
    def __init__(self):
        self._profiles_collection = lambda: ctx.contextManager.db["tbl_profiles"]
        self._merged_profiles_collection = lambda: ctx.contextManager.db["tbl_merged_profiles"]

    async def get_profile_count(self, account_id: str) -> int:
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
        return await self.profiles_collection.count_documents({"account_id": account_id})

    async def get_profile_by_id(self, account_id: str, identifier: str) -> dict:
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
        profile = await self.profiles_collection.find_one({
            "$or": [{"_id": identifier}, {"customer_id": identifier}],
            "account_id": account_id
        })
        
        if profile:
            profile['profile_id'] = str(profile['_id'])
            return await self._format_profile(profile, tag_statuses=['active'])
        return None

    async def get_profiles_by_identifiers(self, account_id: str, identifiers: list, tag_statuses: list, ids: bool = None, internal: bool = False) -> tuple:
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
            cursor = self.profiles_collection.find({
                "$and": [
                    {"$or": [{"_id": {"$in": identifiers}}, {"customer_id": {"$in": identifiers}}]},
                    {"account_id": account_id}
                ]
            }, {"_id": 1})
            async for profile in cursor:
                profile['profile_id'] = str(profile['_id'])
                await self._format_profile(profile, tag_statuses=tag_statuses, internal=internal)
                found_profiles.append(profile)
        else:
            cursor = self.profiles_collection.find({
                "$and": [
                    {"$or": [{"_id": {"$in": identifiers}}, {"customer_id": {"$in": identifiers}}]},
                    {"account_id": account_id}
                ]
            })
            async for profile in cursor:
                profile['profile_id'] = str(profile['_id'])
                await self._format_profile(profile, tag_statuses=tag_statuses, internal=internal)
                found_profiles.append(profile)
        
        # Get all not found IDs
        found_ids = {p['profile_id'] for p in found_profiles}
        for p in identifiers:
            if p not in found_ids:
                not_found.append(p)
        
        return found_profiles, not_found

    async def get_profiles_by_params(self, account_id: str, cursor: int = None, segments: list = None, rfm_values: list = None, churn_prob: str = None) -> tuple:
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
        query = {"account_id": account_id}
        
        if rfm_values:
            query["rfm_score"] = {"$gt": rfm_values[0], "$lt": rfm_values[1]}
        
        if churn_prob:
            query["churn_probability"] = churn_prob
        
        if segments:
            if len(segments) == 1:
                query["segment_tags.segment_tag"] = segments[0]
                query["segment_tags.status"] = "active"
            else:
                query["$or"] = [
                    {"$and": [{"segment_tags.segment_tag": seg}, {"segment_tags.status": "active"}]}
                    for seg in segments
                ]
        
        total = await self.profiles_collection.count_documents(query)
        cursor = self.profiles_collection.find(query).skip(cursor).limit(100)
        profiles = []
        async for profile in cursor:
            profile['profile_id'] = str(profile['_id'])
            await self._format_profile(profile, tag_statuses=['active'])
            profiles.append(profile)
        
        return profiles, total

    async def get_all_profiles(self, account_id: str, tag_statuses: list, cursor: int = None, ids: bool = None, status: str = 'active', limit: int = 100, internal: bool = False) -> tuple:
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
        query = {"account_id": account_id, "status": status}
        projection = {"_id": 1} if ids else None
        
        total = await self.profiles_collection.count_documents(query)
        cursor = self.profiles_collection.find(query, projection).skip(cursor).limit(limit)
        
        profiles = []
        async for profile in cursor:
            profile['profile_id'] = str(profile['_id'])
            await self._format_profile(profile, tag_statuses=tag_statuses, internal=internal)
            profiles.append(profile)
        
        return profiles, total

    async def get_merged_profiles(self, account_id: str, identifiers: list) -> list:
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
        pipeline = []
        for idx, identifier in enumerate(identifiers):
            pipeline.append({
                '$match': {
                    '$and': [
                        {"account_id": account_id},
                        {'$or': [
                            {"merged_profiles.profile_id": identifier},
                            {"merged_profiles.customer_id": identifier},
                            {"parent_profile_id": identifier},
                            {"parent_customer_id": identifier}
                        ]}
                    ]
                }
            })
            pipeline.extend([{'$sort': {'created_at': -1}}, {'$limit': 1}])
        
        results = await self.merged_profiles_collection.aggregate(pipeline).to_list(None)
        merged_profiles = []
        for res in results:
            merged_profiles.append({
                'merged_profiles': res.get('merged_profiles', []),
                'parent_profile_id': res.get('parent_profile_id'),
                'parent_customer_id': res.get('parent_customer_id'),
                'authenticated_id_key': res.get('authenticated_id_key'),
                'authenticated_id_value': res.get('authenticated_id_value'),
                'merged_at': int_to_dt(res['created_at']['$date'], as_str=True) if 'created_at' in res else None
            })
        
        return merged_profiles

    async def create_profiles(self, profiles_batch: list) -> tuple:
        """
        Parameters
        ----------
        profiles_batch : List
            list of profile object dictonaries (valid profile objects)

        Returns
        ----------
        created_profiles, failed_to_create profiles
        """
        operations = []
        customer_ids = []
        
        for profile in profiles_batch:
            profile_doc = {
                "_id": profile['profile_id'],
                "customer_id": profile['customer_id'],
                "account_id": profile['account_id'],
                "profile_data": profile['profile_data'],
                "platform_info": profile['platform_info'],
                "has_charged": profile['has_charged'],
                "created_at": dt.utcnow()
            }
            operations.append(InsertOne(profile_doc))
            customer_ids.append(profile['customer_id'])
        
        try:
            result = await self.profiles_collection.bulk_write(operations, ordered=False)
            created_count = result.inserted_count
        except BulkWriteError as bwe:
            created_count = bwe.details['nInserted']
            invalid = [err['op']['customer_id'] for err in bwe.details['writeErrors']]
        else:
            invalid = []
        
        valid = list(set(customer_ids) - set(invalid))
        failed_to_create = []
        for inv in invalid:
            failed_to_create.append({
                'customer_id': inv,
                'error_message': f'Another profile exists with provided customer_id: {inv}'
            })
        
        created_profiles = []
        for prof in profiles_batch:
            if prof['customer_id'] in valid:
                created = prof.copy()
                created.pop('account_id', None)
                created_profiles.append(created)
        
        return created_profiles, failed_to_create

    async def update_profiles(self, profiles_batch: list, internal: bool) -> tuple:
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
        failed_to_update = []
        profile_ids = [p['profile_id'] for p in profiles_batch]
        
        # Find existing profiles
        existing_profiles = {}
        cursor = self.profiles_collection.find({"_id": {"$in": profile_ids}})
        async for profile in cursor:
            existing_profiles[profile['_id']] = profile
        
        # Prepare bulk operations
        operations = []
        for profile in profiles_batch:
            existing = existing_profiles.get(profile['profile_id'])
            if not existing:
                failed_to_update.append({
                    'profile_id': profile['profile_id'],
                    'error_message': f'No profile found with profile_id: {profile["profile_id"]}'
                })
                continue
            
            update_data = {
                "updated_at": dt.utcnow()
            }
            
            # Update basic fields
            if profile.get('customer_id') is not None:
                update_data['customer_id'] = profile['customer_id']
            if profile.get('profile_data') is not None:
                update_data['profile_data'] = profile['profile_data']
            if profile.get('platform_info') is not None:
                update_data['platform_info'] = profile['platform_info']
            if profile.get('has_charged') is not None:
                update_data['has_charged'] = profile['has_charged']
            if profile.get('status') is not None:
                update_data['status'] = profile['status']
            
            # Update internal fields
            if internal:
                if profile.get('rfm_score') is not None:
                    update_data['rfm_score'] = profile['rfm_score']
                if profile.get('rfm_segment_desc') is not None:
                    update_data['rfm_segment_desc'] = profile['rfm_segment_desc']
                if profile.get('churn_probability') is not None:
                    update_data['churn_probability'] = profile['churn_probability']
                if profile.get('ltv_prediction') is not None:
                    update_data['ltv_prediction'] = profile['ltv_prediction']
                if profile.get('current_ltv') is not None:
                    update_data['current_ltv'] = profile['current_ltv']
                if profile.get('segment_tags') is not None:
                    update_data['segment_tags'] = await self._format_segment_tags(
                        profile['segment_tags'], 
                        existing.get('segment_tags', [])
                    )
            
            operations.append(
                UpdateOne(
                    {"_id": profile['profile_id'], "account_id": existing['account_id']},
                    {"$set": update_data}
                )
            )
            
            # Prepare response object
            updated = existing.copy()
            updated.update(update_data)
            updated['profile_id'] = str(updated['_id'])
            await self._format_profile(updated, tag_statuses=['active'], internal=internal)
            updated_profiles.append(updated)
        
        # Execute bulk operations
        try:
            await self.profiles_collection.bulk_write(operations, ordered=False)
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                profile_id = err['op']['q']['_id']
                failed_to_update.append({
                    'profile_id': profile_id,
                    'error_message': f'Update failed: {err["errmsg"]}'
                })
                # Remove from updated list
                updated_profiles = [p for p in updated_profiles if p['profile_id'] != profile_id]
        
        return updated_profiles, failed_to_update

    async def delete_profiles(self, profiles_batch: list) -> tuple:
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
        deleted_profiles = []
        failed_to_delete = []
        profile_ids = [p['profile_id'] for p in profiles_batch]
        
        # Find existing profiles
        existing_profiles = {}
        cursor = self.profiles_collection.find({"_id": {"$in": profile_ids}})
        async for profile in cursor:
            existing_profiles[profile['_id']] = profile
        
        # Prepare operations
        operations = []
        for profile in profiles_batch:
            if profile['profile_id'] not in existing_profiles:
                failed_to_delete.append({
                    'profile_id': profile['profile_id'],
                    'error_message': f'No profile found with profile_id: {profile["profile_id"]}'
                })
                continue
            
            operations.append(
                DeleteOne({
                    "_id": profile['profile_id'],
                    "account_id": profile['account_id']
                })
            )
            deleted_profiles.append({
                'profile_id': profile['profile_id'],
                'customer_id': existing_profiles[profile['profile_id']]['customer_id']
            })
        
        # Execute operations
        if operations:
            await self.profiles_collection.bulk_write(operations, ordered=False)
        
        return deleted_profiles, failed_to_delete

    async def update_delete_segment_tags(self, account_id: str, segment_ids: list, action: str) -> None:
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
            operations = []
            for seg in segment_ids:
                operations.append(
                    UpdateMany(
                        {
                            "account_id": account_id,
                            "segment_tags.segment_id": seg['segment_id']
                        },
                        {
                            "$set": {
                                "segment_tags.$.status": "pending_deletion",
                                "segment_tags.$.updated_at": dt.utcnow()
                            }
                        }
                    )
                )
            if operations:
                await self.profiles_collection.bulk_write(operations)
        
        elif action == 'delete':
            for seg in segment_ids:
                await self.profiles_collection.update_many(
                    {"account_id": account_id},
                    {"$pull": {"segment_tags": {"segment_id": seg['segment_id']}}}
                )

    async def delete_all_profiles(self, account_id: str) -> bool:
        """
        Parameters
        ----------
        account_id : str
            octy account id

        Returns
        ----------
        bool
        """
        try:
            await self.profiles_collection.delete_many({"account_id": account_id})
            await self.merged_profiles_collection.delete_many({"account_id": account_id})
            await ctx.redis_conn.delete(f'{account_id}_profile_key_types')
            return True
        except Exception as e:
            raise Exception(f"Error deleting profiles: {str(e)}")

    async def create_segment_tags(self, account_id: str, profile_id: str, segment_tags: list) -> None:
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
        operations = []
        for seg in segment_tags:
            operations.append(
                UpdateOne(
                    {
                        "_id": profile_id,
                        "account_id": account_id
                    },
                    {
                        "$push": {
                            "segment_tags": {
                                "segment_id": seg['segment_id'],
                                "segment_tag": seg['segment_tag'],
                                "status": seg['status'],
                                "created_at": dt.utcnow()
                            }
                        }
                    }
                )
            )
        if operations:
            await self.profiles_collection.bulk_write(operations)

    async def update_segment_tags(self, account_id: str, profile_id: str, segment_tags: list) -> None:
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
        await self.profiles_collection.update_one(
            {"_id": profile_id, "account_id": account_id},
            {
                "$set": {
                    "segment_tags": segment_tags,
                    "updated_at": dt.now(tz.utc)
                }
            }
        )

    async def delete_segment_tags(self, account_id: str, profile_id: str, segment_tags: list) -> None:
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
        segment_ids = [seg['segment_id'] for seg in segment_tags]
        await self.profiles_collection.update_one(
            {"_id": profile_id, "account_id": account_id},
            {
                "$pull": {
                    "segment_tags": {"segment_id": {"$in": segment_ids}}
                }
            }
        )

    async def set_profile_key_type(self, account_id: str, profile_key_type: dict) -> None:
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
        await ctx.redis_conn.sadd(f'{account_id}_profile_key_types', json.dumps(profile_key_type))

    async def get_profile_key_types(self, account_id: str) -> list:
        """
        Parameters
        ----------
        account_id : str
            octy account id

        Returns
        ----------
        list
        """
        keys = await ctx.redis_conn.smembers(f'{account_id}_profile_key_types')
        return [json.loads(key) for key in keys]

    async def _format_profile(self, profile: dict, tag_statuses: list, internal: bool = False) -> dict:
        """
          Format profile return objects
        """
    
        if '_id' in profile:
            profile['profile_id'] = str(profile['_id'])
            del profile['_id']
        
        if 'account_id' in profile:
            del profile['account_id']
        
        if not internal:
            profile.pop('ltv_prediction', None)
            profile.pop('current_ltv', None)
        
        # Format segment tags
        if 'segment_tags' in profile:
            valid_tags = []
            for tag in profile['segment_tags']:
                if tag.get('status') not in tag_statuses:
                    continue
                if not internal:
                    tag.pop('segment_id', None)
                    tag.pop('status', None)
                    tag.pop('updated_at', None)
                    if 'created_at' in tag and isinstance(tag['created_at'], dict):
                        tag['created_at'] = int_to_dt(tag['created_at']['$date'], as_str=True)
                valid_tags.append(tag)
            profile['segment_tags'] = valid_tags
        
        # Format dates
        if 'created_at' in profile and isinstance(profile['created_at'], dict):
            profile['created_at'] = int_to_dt(profile['created_at']['$date'], as_str=True)
        if 'updated_at' in profile and isinstance(profile['updated_at'], dict):
            profile['updated_at'] = int_to_dt(profile['updated_at']['$date'], as_str=True)
        
        return profile

    async def _format_segment_tags(self, new_tags: list, existing_tags: list) -> list:
        """
          Updating profile segment tags
        """
        formatted = existing_tags.copy()
        for tag in new_tags:
            found = next((t for t in formatted if t.get('segment_id') == tag.get('segment_id')), None)
            if found:
                found.update(tag)
                found['updated_at'] = dt.utcnow()
            else:
                tag['created_at'] = dt.utcnow()
                formatted.append(tag)
        return formatted




profilesRepository = _ProfilesRepository()