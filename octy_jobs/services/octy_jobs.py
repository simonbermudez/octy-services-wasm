# module imports
from .scheduler import RobustScheduler
from data.repositories.implementation.octy_jobs_repository import octyJobsRepository
from utils.utils import *

# python imports
from typing import *
from datetime import datetime as dt
import asyncio
from functools import reduce
import os
import numbers

# external imports
from octy_rabbitmq.amqp_publisher import amqpPublisher
from sentry_sdk import capture_exception


class OctyJobQueueService():
    """
        OctyJobQueueService
        Handles:
        - Creating new octy jobs
        - Delete octy jobs
        - Get octy jobs
        ...

        Attributes
        ----------
        account_id : str
    """

    def __init__(self, account_id : str) : 
        self.account_id = account_id

    async def create_new_job(self, octy_job : object) -> None:

        new_octy_job = {
            'octy_job_id' : generate_uid('octy-job'),
            'account_id' : self.account_id,
            'alt_dentifier' : octy_job.alt_dentifier,
            'job_meta' : {
                'required_permissions' : octy_job.job_meta.required_permissions,
                'required_configurations' : octy_job.job_meta.required_configurations,
                'amqp_routing_key' : octy_job.job_meta.amqp_routing_key,
                'job_type' : octy_job.job_meta.job_type,
                'desired_runs' : octy_job.job_meta.desired_runs if octy_job.job_meta.desired_runs != 0 else 1000000000000,
                'time_interval' : octy_job.job_meta.time_interval,
                'fail_threshold' : octy_job.job_meta.fail_threshold if octy_job.job_meta.fail_threshold != 0 else 1000000000000,
            },
            'job_data' : octy_job.job_data
        }
        await octyJobsRepository.create_octy_job(self.account_id, new_octy_job)

    async def delete_octy_jobs(self, octy_job_ids : list, alt_identifiers : list) -> None:
        #merge all provided identifers
        octy_job_ids.extend(alt_identifiers)
        await octyJobsRepository.delete_octy_jobs([self.account_id], octy_job_ids)

    #Delete all octy jobs for an account . also delete from queue
    async def delete_all_octy_jobs(self) -> bool:
        await octyJobsRepository.delete_all_octy_jobs(self.account_id)

        amqpPublisher.publish(
            exchange_name='octy_jobs',
            routing_key='octy-job-delete-queue',
            body=json.dumps({'account_id' : self.account_id})
        )
        return True

    async def get_jobs(self) -> list:
        pass

    async def status_callback(self, octy_job : dict) -> None:
        await octyJobsRepository.update_octy_job([
            {
                'account_id' : self.account_id,
                'octy_job_id' : octy_job['octy_job_id'],
                'suc_inc_by' : 1 if octy_job['status'] == 'success' else 0,
                'fail_inc_by' : 1 if octy_job['status'] == 'failed' else 0,
                'status' : 'pending', # update back to pending for next tick to manage
                'action' : 'http callback --> updated job status'
            }
        ])


class OctyJobQueue():
    """
        OctyJobQueue
        Handles:
        - Starting a robust job queue on background thread
        - Graceful stopping of robust job queue on background thread
        - Processing pending octy jobs
        ...

        Attributes
        ----------
        logger : object
        queue_process_interval : int
            the interval (in minutes) at which the job queue should be processed
    """
    def __init__(self, logger : object, queue_process_interval : int): 
        self.logger = logger
        self.queue_process_interval = queue_process_interval # 2 minutes
        self.stop_run_continuously = None
        self.scheduler = RobustScheduler(self.logger)
        self.is_processing = False
        self.pending_jobs = None
        self.accounts = None

    async def run_continuously(self, scheduler):
        while not self.stop_run_continuously:
            await scheduler.run_pending()
            await asyncio.sleep(1)

    async def start_job_queue(self) -> None:
        self.logger.info(f"Octy Job Queue >> Starting robust job queue")
        #Schedule queue process task
        self.scheduler.every(self.queue_process_interval).minutes.do(self._process_octy_jobs)
        loop = asyncio.get_event_loop()
        loop.create_task(self.run_continuously(self.scheduler))
        self.logger.info(f"Octy Job Queue >> Started robust job queue on app aynscio loop!")

    async def stop_job_queue(self):
        #NOTE: Other actions to enable a graceful queue shutdown.. Complete any outsanding messages first!
        self.logger.warning(f"Octy Job Queue >> Stopping robust job queue")
        self.stop_run_continuously = True
        self.logger.info(f"Octy Job Queue >> Stopped robust job queue on background thread!")
    
    # <process octy jobs private methods>
    async def _validate_algorithm_config(self, account : dict, idx : int):
        if not bool(account['algorithm_configurations'][idx]['config_json']):
            return False
        else: return True

    async def _reset_jobs(self, jobs : list) -> None:
        octy_job_updates = list()
        for job in jobs:
            octy_job_updates.append(
                {
                    'account_id' : job['account_id'],
                    'octy_job_id' : job['_id'],
                    'suc_inc_by' : 0,
                    'fail_inc_by' : 0,
                    'status' : 'pending', # update back to pending for next tick to manage
                    'action' : f'octy job queue --> Error occurred during processing :: job status hung as "processing" for more than 24 hours.'
                }
            )
        await octyJobsRepository.update_octy_job(octy_job_updates)

    async def _filter_pending_exceeded_jobs(self, jobs :list) -> Union[list, list]:
        pending_jobs = list(filter(lambda x : x['job_meta']['successful_runs'] < x['job_meta']['desired_runs'] and x['job_meta']['status'] == 'pending' , jobs))
        exceeded_jobs = list(filter(lambda x : x['job_meta']['successful_runs'] >= x['job_meta']['desired_runs'], jobs))
        failed_jobs = list(filter(lambda x : x['job_meta']['failed_runs'] > x['job_meta']['fail_threshold'], jobs))
        processing_jobs = list(filter(lambda x : x['job_meta']['status'] == 'processing', jobs))

        hung_jobs = list()
        for pro_j in processing_jobs:
            last_run_date = int_to_dt(pro_j['job_meta']['last_run']['$date'], as_str=False)
            if (dt.now() - last_run_date).days >= 1:
                # Jobs that have remained in a 'processing' state for more than 24 hours.
                # This signals that the job has hung and will not be run again until 
                # the job status is updated.
                hung_jobs.append(pro_j)
        if len(hung_jobs)>0:
            await self._reset_jobs(hung_jobs)

        p_js=list()
        for j in pending_jobs:

            try:
                last_run_date = j['job_meta']['last_run']['$date']
            except TypeError:
                last_run_date = None

            if not last_run_date:
                last_run = int_to_dt(j['job_meta']['created_at']['$date'], as_str=False)
            else:
                last_run = int_to_dt(j['job_meta']['last_run']['$date'], as_str=False)

            if j['job_meta']['time_interval'] == 0:
                p_js.append(j)
            else:
                #COMPARE MINUTES
                if round((dt.now() - last_run).total_seconds() / 60) >= j['job_meta']['time_interval']:
                    p_js.append(j)
        
        invalid_jobs = list()
        invalid_jobs.extend(exceeded_jobs)
        invalid_jobs.extend(failed_jobs)
        return p_js, invalid_jobs

    async def _get_all_jobs(self) -> list:
        jobs = []
        cursor = 0
        cursor_exhausted = False
        while not cursor_exhausted:
            jobs_page = await octyJobsRepository.get_octy_jobs(cursor)
            num_jobs = len(jobs_page)
            if num_jobs < 1:
                cursor_exhausted = True
            jobs.extend(jobs_page)
            cursor += num_jobs
        return jobs
    
    async def _delete_invalid_jobs(self, invalid_jobs : list) -> None:
        identifiers = []
        account_ids = []
        for er in invalid_jobs:
            identifiers.append(er['_id'])
            account_ids.append(er['account_id'])
        await octyJobsRepository.delete_octy_jobs(account_ids=account_ids,identifiers=identifiers)

    async def _get_accounts(self, account_ids : list) -> None:
        self.accounts = await octyJobsRepository.get_pending_job_accounts(account_ids)
        if not self.accounts:
            # HTTP error must have occurred or all jobs invalid. 
            # let them re-run, but increment failed count
            ex = 'Pending jobs found, but no accounts were returned from account service. Trying again.'
            for octy_job in self.pending_jobs:
                await octyJobsRepository.update_octy_job([
                    {
                        'account_id' : octy_job['account_id'],
                        'octy_job_id' : octy_job['_id'],
                        'suc_inc_by' : 0,
                        'fail_inc_by' : 1,
                        'status' : 'pending', # update back to pending for next tick to manage
                        'action' : f'octy job queue --> Error occurred during processing :: {ex}'
                    }
                ])
            self.is_processing = False
            self.logger.error(f"Octy Job Queue >> {ex}")
            raise Exception(f"Octy Job Queue >> {ex}")
    
    def _deep_get(self, dictionary, keys, default=None):
        return reduce(lambda d, key: d.get(key, default) if isinstance(d, dict) else default, keys.split("."), dictionary)

    async def _validate_account_attr(self, account, key) -> Union[bool, str]: # result & location : 'tl' or 'nest'
        val = self._deep_get(account,key)
        if '.' in key:
            if not isinstance(val, numbers.Number):
                return bool(self._deep_get(account,key)), 'nest'
            else:
                return True, 'nest'
        try:
            if not isinstance(val, numbers.Number):
                return bool(account[key]), 'tl'
            else:
                return True, 'tl'
        except KeyError:
            pass

    async def _validate_algorithm_config(self, account : dict, idx : int) -> bool:
        try:
            return bool(account['algorithm_configurations'][idx]['config_json'])
        except KeyError:
            return False

    async def _build_message_payload(self, account : dict, job : dict) -> dict:
        '''
        Check account has required permissions for this job type.
        Map job to model and return message payload

        Returns 
        -------
        result : bool
        payload : dict
        '''

        # Hard coded required payload attributes
        payload = {
            'account_data' : {
                'account_id' : account['_id']
            },
            'octy_job_id' : job['_id'],
            'job_data' : job['job_data']
        }

        # Assess permissions
        if len(job['job_meta']['required_permissions'])>0:
            for per in job['job_meta']['required_permissions']:
                if per not in account['permissions']:
                    return False, {}

        # Assess required account attributes & configurations	
        for req_acc_attr in job['job_meta']['required_configurations']['account_attributes']:
            res, loc = await self._validate_account_attr(account, req_acc_attr)
            if not res:
                return False, {}
            # append to payload::account_data
            if loc == 'tl':
                payload['account_data'][req_acc_attr] = account[req_acc_attr]
            elif loc == 'nest':
                payload['account_data'][req_acc_attr.split('.')[-1:][0]] = self._deep_get(account,req_acc_attr)
        
        # Assess required algorithm configurations
        for req_algo_conf_idx in \
            job['job_meta']['required_configurations']['algorithm_configuration_idxs']:
            res = await self._validate_algorithm_config(account, req_algo_conf_idx)
            if not res:
                return False, {}
            # append to payload::account_data
            payload['account_data']['algorithm_configurations'] = \
                account['algorithm_configurations'][req_algo_conf_idx]['config_json']
        return True, payload

    async def _process_octy_jobs(self):
        try:
            if self.is_processing:
                self.logger.warning(f"Octy Job Queue >> Not finished processing current batch of Octy jobs.. waiting till next tick")
                return 
            self.is_processing = True
            self.logger.info(f"Octy Job Queue >> Processing Octy jobs")

            jobs = await self._get_all_jobs()
            if len(jobs) < 1:
                self.is_processing = False
                self.logger.warning(f"Octy Job Queue >> No pending jobs found! Going back to sleep zzz")
                return
            
            # Filter pending and invalid jobs
            self.pending_jobs, invalid_jobs = await self._filter_pending_exceeded_jobs(jobs)
            if invalid_jobs:
                await self._delete_invalid_jobs(invalid_jobs=invalid_jobs)

            if len(self.pending_jobs) < 1:
                self.is_processing = False
                self.logger.warning(f"Octy Job Queue >> No pending jobs found! Going back to sleep zzz")
                return

            # Get account data for all accounts associated with pending jobs.
            await self._get_accounts(account_ids=[job['account_id'] for job in self.pending_jobs])

            octy_job_updates = list()
            for job in self.pending_jobs:

                is_job_owner = await octyJobsRepository.claim_pending_job(account_id=job['account_id'], 
                                                                         octy_job_id=job['_id'],
                                                                         pod_id=os.environ.get('POD_ID'))
                if not is_job_owner:
                    self.logger.warning(f"Octy Job Queue >> pending job with ID: {job['_id']} is not owned by this pod. Skipping job.")
                    continue

                account = next((key for key in self.accounts if key['_id'] == job['account_id']), None)
                if not account:
                    octy_job_updates.append([
                        {
                            'account_id' : job['account_id'],
                            'octy_job_id' : job['_id'],
                            'suc_inc_by' : 0,
                            'fail_inc_by' : 1,
                            'status' : 'pending', # update back to pending for next tick to manage
                            'action' : f'octy job queue --> Error occurred during processing :: {job["account_id"]} was not returned by the account service.'
                        }
                    ])
                    continue

                res, payload = await self._build_message_payload(account, job)
                if res:
                    await amqpPublisher.send_message(routing_key=job['job_meta']['amqp_routing_key'], payload=payload)
                else:
                    self.logger.warning(f'Job failed to be processed due to missing attributes. Account ID : {account["_id"]} Job type : {job["job_meta"]["job_type"]}')
                    continue

                # update job to 'processing' to ensure future ticks do not run it again
                octy_job_updates.append(
                    {
                        'account_id' : account['_id'],
                        'octy_job_id' : job['_id'],
                        'suc_inc_by' : 0,
                        'fail_inc_by' : 0,
                        'status' : 'processing',
                        'action' : 'octy job queue --> processing job'
                    }
                )

            if len(octy_job_updates) > 0:
                await octyJobsRepository.update_octy_job(octy_job_updates)
            self.is_processing = False
        except Exception as e:
            capture_exception(e)
            self.is_processing = False
            raise Exception(e)
