# module imports
from data.repositories.implementation.account_repository import accountRepository
from data.models.account import UpdateAccount

# python imports
import threading
import asyncio
import functools
import json
import logging

# external imports
from aio_pika.exceptions import MessageProcessError

logger = logging.getLogger('uvicorn.error')
sem = threading.BoundedSemaphore(10)


def ack_message(payload, did_succeed : bool=True, requeue : bool=True) -> None:
    try:
        if did_succeed:
            payload.ack()
        else:
            payload.reject(requeue=requeue)
        logger.info(f"Acknowledged message! Did succeed: {did_succeed} Requeued message: {requeue}")
    except MessageProcessError:
        logger.error("Failed to acknowledge message!")
        payload.reject(requeue=False)

def handle_message(payload, main_loop) -> None:
    sem.acquire()
    routing_key = payload.routing_key
    logger.info(f'Thread id: {threading.get_ident()} Delivery tag: {payload.delivery_tag} Message ID: {payload.message_id} Routing Key : {routing_key}')

    try:
        message_json = json.loads(payload.body.decode())
        account = UpdateAccount(**message_json)
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
        if routing_key == 'account.configs.cmd.update':
            loop.run_until_complete(accountRepository.update_account(account, 'account-config'))
        elif routing_key == 'algo.configs.cmd.update':
            loop.run_until_complete(accountRepository.update_account(account, 'algorithm-config'))
        elif routing_key == 'churn.info.cmd.update':
            loop.run_until_complete(accountRepository.update_account(account, 'churn-info'))
    except Exception as ex:
        logger.error(f'Error updating account: {ex}')
        # Requeue failed message
        cb = functools.partial(ack_message, payload, did_succeed=False, requeue=False if '[toxic]::' in str(ex) else True)
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