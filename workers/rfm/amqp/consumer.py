# module imports
from data.models.rfm_jobs import RFMAnalysisJob, RFMCompleteJob
from services.rfm import RFMAnalysis, RFMCompleteAnalysis

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
        if routing_key == 'rfm.training.cmd.run':
            job_payload = RFMAnalysisJob(**message_json)
        elif routing_key == 'rfm.training.complete.cmd.run':
            job_payload = RFMCompleteJob(**message_json)
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
        if routing_key == 'rfm.training.cmd.run':
            loop.run_until_complete(RFMAnalysis(account_id=job_payload.account_data.account_id, 
                                    octy_job_id=job_payload.octy_job_id,
                                    bucket=job_payload.account_data.bucket,
                                    loop=main_loop).run())
        
        elif routing_key == 'rfm.training.complete.cmd.run':
            loop.run_until_complete(RFMCompleteAnalysis(account_id=job_payload.account_data.account_id,
                                    octy_job_id=job_payload.octy_job_id,
                                    bucket=job_payload.account_data.bucket,
                                    training_job_id=job_payload.job_data.training_job_id,
                                    webhook_url=job_payload.account_data.webhook_url,
                                    loop=main_loop).run())
    except Exception as ex:
        logger.error(f'Error running rfm job: {ex}')
        # Requeue failed message
        cb = functools.partial(ack_message, payload, False, False) # Allow Octy Job Scheduler to reshcedule job
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