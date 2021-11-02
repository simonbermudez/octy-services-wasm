# module imports
from data.repositories.Iocty_jobs_repository import OctyJobsInterface
from utils.utils import *
from api.routers.error_handlers import *
from data.models.db_schemas import tbl_octy_jobs, JobMeta, RequiredConfigs

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import time

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps


class _OctyJobsRepository(OctyJobsInterface):
    """
        _OctyJobsRepository
        Handles:
        - Creating new octy jobs
        - Delete octy jobs
        - Get octy jobs
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass


    async def create_octy_job(self, account_id : str, octy_job : dict) -> None:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        octy_job : dict

        Returns
        ----------
        None
        """
        #TODO: Do not create duplicate Job unless current matching job has exceeded desited run or failed limits etc.
        required_configurations = RequiredConfigs(
            account_attributes=octy_job['job_meta']['required_configurations'].account_attributes,
            algorithm_configuration_idxs=octy_job['job_meta']['required_configurations'].algorithm_configuration_idxs
        )
        job_meta = JobMeta(
            job_type=octy_job['job_meta']['job_type'],
            amqp_routing_key=octy_job['job_meta']['amqp_routing_key'],
            required_permissions=octy_job['job_meta']['required_permissions'],
            required_configurations=required_configurations,
            desired_runs=octy_job['job_meta']['desired_runs'],
            time_interval=octy_job['job_meta']['time_interval'],
            fail_threshold=octy_job['job_meta']['fail_threshold']
        )
        
        db_job = tbl_octy_jobs(
            octy_job_id=octy_job['octy_job_id'],
            account_id=account_id,
            alt_dentifier=octy_job['alt_dentifier'],
            job_meta=job_meta,
            job_data=octy_job['job_data']
        )
        db_job.save()
    
    async def update_octy_job(self, octy_job_updates : list) -> None:
        """
        Parameters
        ----------
        octy_job_updates : list

        Returns
        ----------
        None
        """
        # determine the correct status given the jobs current attributes

        bulk_operation = tbl_octy_jobs._get_collection().initialize_unordered_bulk_op()
        for job in octy_job_updates:
            if job['status'] == 'processing':

                bulk_operation.find({
                    '$and' : [
                        {"account_id" : { "$eq" : job['account_id']} },
                        {"_id" : { "$eq" : job['octy_job_id']} }
                    ]
                }).update(
                    {
                        "$set" : 
                            {   
                                "job_meta.status":job['status'],
                                "job_meta.last_run":dt.now(),
                                "job_meta.updated_at":dt.now(),
                                "job_meta.last_updated_action":job['action']
                            }
                    }
                )
            else:
                bulk_operation.find({
                    '$and' : [
                        {"account_id" : { "$eq" : job['account_id']} },
                        {"_id" : { "$eq" : job['octy_job_id']} }
                    ]
                }).update(
                    {
                        "$inc" : {
                            "job_meta.successful_runs": job['suc_inc_by'], # 0 if updating other attibutes of job. 1 If successful run
                            "job_meta.failed_runs": job['fail_inc_by']
                        },
                        "$set" : 
                            {   
                                "job_meta.status":job['status'],
                                "job_meta.updated_at":dt.now(),
                                "job_meta.last_updated_action":job['action']
                            }
                    }
                )
        bulk_operation.execute()

    async def delete_octy_jobs(self, account_ids : list, identifiers : list) -> None:
        """
        Parameters
        ----------
        account_id : list
            Octy account id
        identifiers : list

        Returns
        ----------
        None
        """
        bulk_operation = tbl_octy_jobs._get_collection().initialize_unordered_bulk_op()
        for iden in identifiers:
            if 'octy-job_' in iden:
                bulk_operation.find({
                    '$and' : [
                        {  "_id" : { "$eq" : iden }  },
                        {  "account_id" : { "$in" : account_ids }  }
                    ]
                }).remove()
            else:
                bulk_operation.find({
                    '$and' : [
                        {  "alt_dentifier" : { "$eq" : iden }  },
                        {  "account_id" : { "$in" : account_ids }  }
                    ]
                }).remove()
        bulk_operation.execute()

    async def get_octy_jobs(self, cursor : int) -> list:
        """
        Parameters
        ----------
        cursor : int

        Returns
        ----------
        jobs : list
        """
        results_cursor = tbl_octy_jobs._get_collection().find().sort("job_meta.created_at", 1).skip(cursor).limit(1000)
        raw_res = json.loads(dumps(list(results_cursor), indent = 2))
        return raw_res

    async def get_pending_job_accounts(self, account_ids : list) -> list:
        """
        Parameters
        ----------
        account_ids : list

        Returns
        ----------
        list
        """
        url = f"{Config['ACCOUNT_SERVICE_CLUSTER_IP']}/v1/internal/accounts"
        accounts = []
        exhausted_accounts = False
        cursor : int = 0
        session = requests_retry_session()

        payload = {
            'account_ids' : account_ids
        }

        while not exhausted_accounts:
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
                exhausted_accounts = True
                continue

            body = json.loads(response.text)
            for account in body['accounts']:
                accounts.append(
                    account
                )
            cursor += body['request_meta']['count']

        return accounts

octyJobsRepository = _OctyJobsRepository()