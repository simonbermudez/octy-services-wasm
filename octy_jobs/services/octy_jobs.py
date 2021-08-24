# module imports
from .scheduler import RobustScheduler
from data.repositories.implementation.octy_jobs_repository import octyJobsRepository
from .AMQP import amqpInterface
from utils.utils import *
from config import Config

# python imports
from typing import *
import json
from datetime import date
import asyncio

# external imports
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
                'job_type' : octy_job.job_type,
                'desired_runs' : octy_job.job_meta.desired_runs if octy_job.job_meta.desired_runs != 0 else 1000000000000,
                'time_interval' : octy_job.job_meta.time_interval,
                'fail_threshold' : octy_job.job_meta.fail_threshold if octy_job.job_meta.fail_threshold != 0 else 1000000000000,
            },
            'job_data' : {
                'data' : octy_job.job_data,
            }
        }
        await octyJobsRepository.create_octy_job(self.account_id, new_octy_job)

    async def delete_octy_jobs(self, octy_job_ids : list, alt_identifiers : list) -> None:
        #merge all provided identifers
        octy_job_ids.extend(alt_identifiers)
        await octyJobsRepository.delete_octy_jobs([self.account_id], octy_job_ids)

    async def get_jobs(self) -> list:
        pass

    async def status_callback(self, octy_job : dict) -> None:
        octyJobsRepository.update_octy_job([
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
            number in minutes, the job queue should be processed
    """
    def __init__(self, logger : object, queue_process_interval : int): 
        self.logger = logger
        self.queue_process_interval = queue_process_interval # 2 minutes
        self.stop_run_continuously = None
        self.scheduler = RobustScheduler(self.logger)
        self.is_processing = False

    async def run_continuously(self, scheduler):
        while not self.stop_run_continuously:
            await scheduler.run_pending()
            await asyncio.sleep(1)

    async def start_job_queue(self) -> None:
        self.logger.info(f"Octy Job Queue >> Starting robust job queue")
        #Schedule queue process task
        self.scheduler.every(self.queue_process_interval).minutes.do(self.process_octy_jobs)
        #self.stop_run_continuously = run_continuously(self.scheduler)
        loop = asyncio.get_event_loop()
        loop.create_task(self.run_continuously(self.scheduler))
        self.logger.info(f"Octy Job Queue >> Started robust job queue on app aynscio loop!")

    async def stop_job_queue(self):
        #NOTE: Other actions to enable a graceful queue shutdown.. Complete any outsanding messages first!
        self.logger.warning(f"Octy Job Queue >> Stopping robust job queue")
        self.stop_run_continuously = True
        self.logger.info(f"Octy Job Queue >> Stopped robust job queue on background thread!")
    
    async def validate_algorithm_config(self, account : dict, idx : int):
        print(account['algorithm_configurations'][idx]['config_json'])
        if not bool(account['algorithm_configurations'][idx]['config_json']):
            return False
        else: return True

    async def filter_pending_exceeded_jobs(self, jobs :list) -> Union[list, list]:
        p_js=[]
        pending_jobs = list(filter(lambda x : x['job_meta']['successful_runs'] < x['job_meta']['desired_runs'] and x['job_meta']['status'] == 'pending' , jobs))
        exceeded_jobs = list(filter(lambda x : x['job_meta']['successful_runs'] >= x['job_meta']['desired_runs'], jobs))
        failed_jobs = list(filter(lambda x : x['job_meta']['failed_runs'] > x['job_meta']['fail_threshold'], jobs))
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

        invalid_jobs = []
        invalid_jobs.extend(exceeded_jobs)
        invalid_jobs.extend(failed_jobs)
        return p_js, invalid_jobs

    async def process_octy_jobs(self):
        try:
            if self.is_processing:
                self.logger.warning(f"Octy Job Queue >> Not finished processing current batch of Octy jobs.. waiting till next tick")
                return 

            self.is_processing = True

            self.logger.info(f"Octy Job Queue >> Processing Octy jobs")

            jobs = []
            cursor = 0
            cursor_exhausted = False

            while not cursor_exhausted:
                jobs_page = octyJobsRepository.get_octy_jobs(cursor)
                num_jobs = len(jobs_page)
                if num_jobs < 1:
                    cursor_exhausted = True
                jobs.extend(jobs_page)
                cursor += num_jobs
            
            if len(jobs) < 1:
                self.is_processing = False
                self.logger.warning(f"Octy Job Queue >> No pending jobs found! Going back to sleep zzz")
                return
            
            pending_jobs, invalid_jobs = await self.filter_pending_exceeded_jobs(jobs)

            if invalid_jobs:
                identifiers = []
                account_ids = []
                for er in invalid_jobs:
                    identifiers.append(er['_id'])
                    account_ids.append(er['account_id'])
                await octyJobsRepository.delete_octy_jobs(account_ids=account_ids,identifiers=identifiers)

            if len(pending_jobs) < 1:
                self.is_processing = False
                self.logger.warning(f"Octy Job Queue >> No pending jobs found! Going back to sleep zzz")
                return

            # Get account data for all accounts associated with pending jobs.
            account_ids = [] 
            for job in pending_jobs: account_ids.append(job['account_id'])
            
            accounts = octyJobsRepository.get_pending_job_accounts(account_ids)
            if not accounts:
                # HTTP error must have occurred or all jobs invalid. 
                # let them re-run, but increment failed count
                ex = 'Pending jobs found, but no accounts were returned from account service. Trying again.'
                for octy_job in pending_jobs:
                    octyJobsRepository.update_octy_job([
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
                capture_exception(Exception(f"Octy Job Queue >> {ex}"))
                raise Exception(f"Octy Job Queue >> {ex}")

            octy_job_updates = []
            for job in pending_jobs:

                account = next((key for key in accounts if key['_id'] == job['account_id']), None)
                if not account:
                    octyJobsRepository.update_octy_job([
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
                #switch through job type to complete required action
                if job['job_meta']['job_type'] == 'seg' : 
                    # Check permissions from account
                    if 'seg' not in account['permissions']:
                        continue
                    #switch through segmentation types
                    if job['job_data']['data']['segmentation_type'] == 'past' :
                        await amqpInterface.publish_message(routing_key='past.segmentation.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'segment_data' : {
                                    'segmentation_type' : job['job_data']['data']['segmentation_type'],
                                    'segment_id' : job['job_data']['data']['segment_id']
                                },
                                'octy_job_id' : job['_id']   
                            })

                    elif job['job_data']['data']['segmentation_type'] == 'live' :
                        await amqpInterface.publish_message(routing_key='live.segmentation.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'segment_data' : {
                                    'segmentation_type' : job['job_data']['data']['segmentation_type']
                                },
                                'event_data' : job['job_data']['data']['event_data'],
                                'octy_job_id' : job['_id'],
                                'validation_job' : False
                            })
                
                    elif job['job_data']['data']['segmentation_type'] == 'pending-live' :
                        await amqpInterface.publish_message(routing_key='live.segmentation.cmd.run',
                            message_payload={
                                'account_data' : {                                
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'segment_data' : {
                                    'segmentation_type' : job['job_data']['data']['segmentation_type'],
                                    'segment_id' : job['job_data']['data']['segment_id']
                                },
                                'event_data' : {
                                    'profile' : {
                                        'profile_id' : job['job_data']['data']['profile_id']
                                    }
                                },
                                'live_octy_job_id' : job['job_data']['data']['live_octy_job_id'],
                                'octy_job_id' : job['_id'],
                                'event_timeframe' : job['job_meta']['time_interval'],
                                'validation_job' : True 
                            })
                
                elif job['job_meta']['job_type'] == 'rec' : 
                    # Check permissions from account
                    if 'rec' not in account['permissions']:
                        continue
                    if not await self.validate_algorithm_config(account, 0):
                        continue
                    #switch through job sub types
                    if job['job_data']['data']['job_sub_type'] == 'training' :
                        await amqpInterface.publish_message(routing_key='rec.training.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'rec_job_data' : {
                                    'bucket' : account['bucket'],
                                    'algorithm_configurations' : account['algorithm_configurations'][0]['config_json']
                                },
                                'octy_job_id' : job['_id']   
                            })

                    elif job['job_data']['data']['job_sub_type'] == 'complete' :
                        await amqpInterface.publish_message(routing_key='rec.training.complete.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'rec_job_data' : {
                                    'hyperparam_tuning_job_id' : job['job_data']['data']['hyperparam_tuning_job_id'],
                                    'bucket' : account['bucket'],
                                    'algorithm_configurations' : account['algorithm_configurations'][0]['config_json']
                                },
                                'octy_job_id' : job['_id']
                            })
                
                elif job['job_meta']['job_type'] == 'churn' : 
                    if 'churn' not in account['permissions']:
                        continue
                    if not await self.validate_algorithm_config(account, 1):
                        continue
                    #switch through job sub types
                    if job['job_data']['data']['job_sub_type'] == 'training':
                        await amqpInterface.publish_message(routing_key='churn.training.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'churn_job_data' : {
                                    'bucket' : account['bucket'],
                                    'algorithm_configurations' : account['algorithm_configurations'][1]['config_json']
                                },
                                'octy_job_id' : job['_id']
                            })

                    if job['job_data']['data']['job_sub_type'] == 'complete': 
                        await amqpInterface.publish_message(routing_key='churn.training.complete.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'churn_job_data' : {
                                    'training_job_id' : job['job_data']['data']['training_job_id'],
                                    'previous_churn_percentage' : account['churn_info']['churn_precentage'],
                                    'bucket' : account['bucket'],
                                    'algorithm_configurations' : account['algorithm_configurations'][1]['config_json']
                                },
                                'octy_job_id' : job['_id']
                            })
                
                elif job['job_meta']['job_type'] == 'rfm' : 
                    if 'rfm' not in account['permissions']:
                        continue
                    #switch through job sub types
                    if job['job_data']['data']['job_sub_type'] == 'training':
                        await amqpInterface.publish_message(routing_key='rfm.training.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'rfm_job_data' : {
                                    'bucket' : account['bucket']
                                },
                                'octy_job_id' : job['_id']
                            })

                    if job['job_data']['data']['job_sub_type'] == 'complete': 
                        await amqpInterface.publish_message(routing_key='rfm.training.complete.cmd.run',
                            message_payload={
                                'account_data' : {
                                    'account_id' : account['_id'],
                                    'webhook_url' : account['account_configurations']['webhook_url'] if account['account_configurations']['webhook_url'] != '' or account['account_configurations']['webhook_url'] != None else 'https://google.com'
                                },
                                'rfm_job_data' : {
                                    'training_job_id' : job['job_data']['data']['training_job_id'],
                                    'bucket' : account['bucket']
                                },
                                'octy_job_id' : job['_id']
                            })
                
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

            octyJobsRepository.update_octy_job(octy_job_updates)
            self.is_processing = False
        except Exception as e:
            capture_exception(e)
            self.is_processing = False
            raise Exception(e)
