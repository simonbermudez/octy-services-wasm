# module imports
from data.repositories.Iversioning_repository import VersioningInterface
from utils.utils import *
import data.context.db_context as ctx


# python imports
from typing import *
import json
from datetime import datetime as dt
import operator

# external imports


class _VersioningRepository(VersioningInterface):
    """
        _VersioningRepository
        Handles:
        - Versioning information cache

        ...

        Attributes
        ----------
        connection : asynchronous redis connection
    """
    def __init__(self): pass


    async def cache_version_data(self, data : dict, repository_name : str) -> None:
        """
        Parameters
        ----------
        data : dict
            The version data that will be cached
        repository_name : str
            The name of the repository version info is being cached for

        Returns
        ----------
        :rtype: None
        """

        version_data = {}
        version_data['id'] = generate_uid(repository_name+'-release')
        version_data['release_id'] = data['id']
        version_data['version_tag'] = data['tag_name']
        version_data['version_name'] = data['name']
        version_data['version_int'] = version_data['version_tag']
        if 'beta' in version_data['version_tag']:
            for char in "v . - b e t a": 
                version_data['version_int'] = version_data['version_int'].replace(char, "")
        elif 'alpha' in version_data['version_tag']:
            for char in "v . - a l p h a": 
                version_data['version_int'] = version_data['version_int'].replace(char, "")
        version_data['version_int'] = int(version_data['version_int'])
        version_data['change_log'] = data['body']
        version_data['assets'] = data['assets']
        version_data['published_at'] = data['published_at']
        version_data['updated_at'] = dt.now().strftime("%m-%d-%YT%H:%M:%S")

        await ctx.redis_conn.sadd(repository_name, json.dumps(version_data))

        

    async def get_cached_version_data(self, key : str) -> object:
        """
            Parameters
            ----------
            key : str
                Key to get version data for

            Returns
            ----------
            :rtype: object
        """
        alpha_versions = []
        beta_versions = []
        stable_versions = []
        sorted_versions = []

        def sort_versions(versions : list) -> list:
            versions_sorted_by_date = sorted(versions, key=operator.itemgetter('updated_at'), reverse=True)
            versions_sorted_by_version = sorted(versions_sorted_by_date, key=operator.itemgetter('version_int'), reverse=True)
            return versions_sorted_by_version

        versions = json.loads(json.dumps([json.loads(s) for s in 
         list(ctx.redis_conn.smembers(key))]))
        
        for version in versions:
            if 'alpha' in version['version_tag']:
                alpha_versions.append(version)
            elif 'beta' in version['version_tag']:
                beta_versions.append(version)
            else:
                stable_versions.append(version)

        sorted_versions.extend(sort_versions(stable_versions))
        sorted_versions.extend(sort_versions(beta_versions))
        sorted_versions.extend(sort_versions(alpha_versions))

        return sorted_versions

versioningRepository = _VersioningRepository()