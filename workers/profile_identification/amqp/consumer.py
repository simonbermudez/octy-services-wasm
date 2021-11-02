# module imports
from data.models.profile_iden_jobs import ProfileIdenJob
from services.profile_identification import ProfileIdentification

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
        job_payload = ProfileIdenJob(**message_json)
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
        if routing_key == 'profile.identification.cmd.run':
            loop.run_until_complete(ProfileIdentification(account_id=job_payload.account_data.account_id, 
                    webhook_url=job_payload.account_data.webhook_url,
                    authenticated_id_key=job_payload.account_data.authenticated_id_key,
                    octy_job_id=job_payload.octy_job_id).run())
    except Exception as ex:
        logger.error(f'Error running profile identification job: {ex}')
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