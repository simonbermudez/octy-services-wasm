# module imports
from services.octy_jobs import OctyJobQueueService
from data.models.octy_jobs import CreateOctyJob, DeleteOctyJob

# python imports
import threading
import asyncio
import functools
import json
import logging

# external imports
from aio_pika.exceptions import MessageProcessError

logger = logging.getLogger('uvicorn')
sem = threading.BoundedSemaphore(10)


def ack_message(payload, did_succeed : bool = True, requeue : bool = True) -> None:
    try:
        if did_succeed:
            payload.ack()
        else:
            payload.reject(requeue=requeue)
        logger.info("Acknowledged message!")
    except MessageProcessError:
        logger.error("Failed to acknowledge message!")
        payload.reject(requeue=False)

def handle_message(payload, main_loop) -> None:
    sem.acquire()
    routing_key = payload.routing_key
    logger.info(f'Thread id: {threading.get_ident()} Delivery tag: {payload.delivery_tag} Message ID: {payload.message_id} Routing Key : {routing_key}')

    try:
        message_json = json.loads(payload.body.decode())
        if routing_key == 'octy.job.cmd.create':
            octy_job = CreateOctyJob(**message_json)
        elif routing_key == 'octy.job.cmd.delete':
            delete_jobs = DeleteOctyJob(**message_json)
    except Exception as ex:
        # if the message_payload is not valid JSON refuse message.
        logger.error(f'Refused message payload: {payload.body.decode()}. Exception : {ex}')
        # Completion callback to main thread async loop
        cb = functools.partial(ack_message, payload, False, False)
        main_loop.call_soon_threadsafe(cb)
        sem.release()
        return

    # Create a new asyncio loop for this worker thread
    # to execute its work on.
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    try:
        if routing_key == 'octy.job.cmd.create':
            loop.run_until_complete(OctyJobQueueService(octy_job.account_id).create_new_job(octy_job=octy_job))
        elif routing_key == 'octy.job.cmd.delete':
            octy_job_ids = delete_jobs.octy_job_ids if delete_jobs.octy_job_ids else []
            alt_identifiers = delete_jobs.alt_identifiers if delete_jobs.alt_identifiers else []
            loop.run_until_complete(OctyJobQueueService(delete_jobs.account_id)\
                .delete_octy_jobs(octy_job_ids=octy_job_ids, alt_identifiers=alt_identifiers))
    except Exception as ex:
        logger.error(f'Error updating or deleting octy jobs: {ex}')
        # Requeue failed message
        cb = functools.partial(ack_message, payload, False, True)
        main_loop.call_soon_threadsafe(cb)
        loop.close() # Close this threads loop
        sem.release()
        return

    loop.close() # Close this threads loop

    # Completion callback to main thread async loop
    cb = functools.partial(ack_message, payload)
    main_loop.call_soon_threadsafe(cb)
    sem.release()


async def on_consumer_message_cb(payload):
    main_loop = asyncio.get_running_loop()
    # handle worker process in background thread
    runthread = threading.Thread(target=handle_message, args=(payload,main_loop,))
    runthread.start()