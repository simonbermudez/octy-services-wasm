# module imports
from data.repositories.Iocty_jobs_repository import OctyJobsInterface
from utils.utils import *
from api.routers.error_handlers import *
from data.models.db_schemas import tbl_octy_jobs, JobMeta, RequiredConfigs
import data.context.db_context as ctx

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
        existing_jobs =  tbl_octy_jobs._get_collection().find({
            '$and' : [
                {"account_id" : { "$eq" : account_id}},
                {"job_meta.job_type" : {"$eq" : octy_job['job_meta']['job_type']}},
                {"job_meta.amqp_routing_key" : {"$eq" : octy_job['job_meta']['amqp_routing_key']}},
                {"job_data" : {"$eq" : octy_job['job_data']}}
            ]
        })

        create_job_ref = True
        if existing_jobs.count()>0:
            create_job_ref = False
            if existing_jobs[0]['job_meta']['successful_runs'] >= existing_jobs[0]['job_meta']['desired_runs'] or \
                existing_jobs[0]['job_meta']['failed_runs'] >= existing_jobs[0]['job_meta']['fail_threshold']:
                # Allow a tempory duplicate if we know the next job tick will handle the deletion of the stale job instance.
                create_job_ref = True

        if create_job_ref:
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

    async def claim_pending_job(self, account_id : str, octy_job_id : str, pod_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        octy_job_id : str
            Octy Job id
        pod_id : str
            K8 pod identifier running this instance

        Returns
        ----------
        is_owner : bool
        """
        '''
        FLOW 1:
            - OctyJobPod1 picks up pending job and claims it. (deleted after 24 hours)
            - OctyJobPod1 processes job.
            - OctyJobPod2 picks up pending job, attmepts to claim it and gets rejected. 
            - OctyJobPod2 skips job, does not report as failed.
        
        FLOW 2: 
            - OctyJobPod1 owns job and a failed response was returned from worker.
            - Next tick, OctyJobPod1 picks up pending job, attmepts to claim it and gets accepted as OctyJobPod1 pod ID matches 
            - OctyJobPod1 processes job.
        
        '''
        name = f'account.id:{account_id}:octy.job:{octy_job_id}'
        res = ctx.redis_conn.\
            set(name=name, 
                value=pod_id, 
                nx=True, 
                ex=86400) # expire after 24 hours
        if not res:
            # This means this job exists and is owned by A pod.
            # Determine if its THIS pod that owns it.
            job_pod_id = ctx.redis_conn.get(name=name)
            if job_pod_id.decode() == pod_id:
                return True
            else:
                return False
        else:
            return True
        

        
octyJobsRepository = _OctyJobsRepository()