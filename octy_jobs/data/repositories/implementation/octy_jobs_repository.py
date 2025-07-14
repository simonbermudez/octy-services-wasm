# module imports
from data.repositories.Iocty_jobs_repository import OctyJobsInterface
from utils.utils import *
from api.routers.error_handlers import *
import data.context.db_context as ctx

# python imports
from typing import *
import json
from datetime import datetime as dt
from datetime import timedelta as td
import time

# external imports
from pymongo import UpdateOne
from bson.json_util import dumps
from bson import ObjectId

class _OctyJobsRepository:
    def __init__(self):
        self.collection = lambda: ctx.contextManager.db["tbl_octy_jobs"]

    async def create_octy_job(self, account_id: str, octy_job: dict):
        query = {
            '$and': [
                {"account_id": account_id},
                {"job_meta.job_type": octy_job['job_meta']['job_type']},
                {"job_meta.amqp_routing_key": octy_job['job_meta']['amqp_routing_key']},
                {"job_data": octy_job['job_data']}
            ]
        }

        existing_jobs = await self.collection().find(query).to_list(length=1)
        create_job_ref = True

        if existing_jobs:
            job = existing_jobs[0]
            if job['job_meta']['successful_runs'] >= job['job_meta']['desired_runs'] or \
               job['job_meta']['failed_runs'] >= job['job_meta']['fail_threshold']:
                create_job_ref = True
            else:
                create_job_ref = False

        if create_job_ref:
            await self.collection().insert_one({
                "octy_job_id": octy_job['octy_job_id'],
                "account_id": account_id,
                "alt_dentifier": octy_job['alt_dentifier'],
                "job_meta": octy_job['job_meta'],
                "job_data": octy_job['job_data']
            })

    async def update_octy_job(self, octy_job_updates: list):
        operations = []

        for job in octy_job_updates:
            match = {
                "account_id": job['account_id'],
                "_id": ObjectId(job['octy_job_id'])
            }

            if job['status'] == 'processing':
                update = {
                    "$set": {
                        "job_meta.status": job['status'],
                        "job_meta.last_run": dt.now(),
                        "job_meta.updated_at": dt.now(),
                        "job_meta.last_updated_action": job['action']
                    }
                }
            else:
                update = {
                    "$inc": {
                        "job_meta.successful_runs": job['suc_inc_by'],
                        "job_meta.failed_runs": job['fail_inc_by']
                    },
                    "$set": {
                        "job_meta.status": job['status'],
                        "job_meta.updated_at": dt.now(),
                        "job_meta.last_updated_action": job['action']
                    }
                }

            operations.append(UpdateOne(match, update))

        if operations:
            await self.collection().bulk_write(operations)

    async def delete_octy_jobs(self, account_ids: list, identifiers: list):
        operations = []
        for iden in identifiers:
            if 'octy-job_' in iden:
                operations.append(UpdateOne(
                    {"_id": iden, "account_id": {"$in": account_ids}}, {"$unset": {}}, upsert=False
                ))
            else:
                operations.append(UpdateOne(
                    {"alt_dentifier": iden, "account_id": {"$in": account_ids}}, {"$unset": {}}, upsert=False
                ))

        if operations:
            await self.collection().delete_many({
                "$or": [
                    {"_id": {"$in": identifiers}},
                    {"alt_dentifier": {"$in": identifiers}}
                ],
                "account_id": {"$in": account_ids}
            })

    async def delete_all_octy_jobs(self, account_id: str):
        await self.collection().delete_many({"account_id": account_id})
        ctx.redis_conn.delete(f'account.id:{account_id}:octy.job:*')
        return True

    async def get_octy_jobs(self, cursor: int) -> list:
        cursor_data = self.collection().find().sort("job_meta.created_at", 1).skip(cursor).limit(1000)
        results = await cursor_data.to_list(length=1000)
        return json.loads(dumps(results))

    async def get_pending_job_accounts(self, account_ids: list) -> list:
        url = f"{Config['ACCOUNT_SERVICE_CLUSTER_IP']}/v1/internal/accounts"
        accounts = []
        exhausted = False
        cursor = 0
        session = requests_retry_session()
        payload = {'account_ids': account_ids}

        while not exhausted:
            try:
                response = session.post(
                    url,
                    data=json.dumps(payload),
                    headers={'cursor': str(cursor)},
                    timeout=60
                )
            except Exception as e:
                raise Exception(str(e)) from None

            if response.status_code != 200:
                exhausted = True
                continue

            body = json.loads(response.text)
            accounts.extend(body['accounts'])
            cursor += body['request_meta']['count']

        return accounts

    async def claim_pending_job(self, account_id: str, octy_job_id: str, pod_id: str) -> bool:
        name = f'account.id:{account_id}:octy.job:{octy_job_id}'
        res = ctx.redis_conn.set(name=name, value=pod_id, nx=True, ex=86400)
        if not res:
            job_pod_id = ctx.redis_conn.get(name=name)
            return job_pod_id.decode() == pod_id
        return True


octyJobsRepository = _OctyJobsRepository()    

    