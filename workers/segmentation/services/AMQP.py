# module imports
from utils.utils import *
from config import *

# python imports
import asyncio
from typing import *
import json
import time

# external imports
from aio_pika import connect as connect_robust, ExchangeType, Message, DeliveryMode, AMQPException, RobustChannel
from aiormq.exceptions import *


class AMQPInterface(): 
    """
        AMQPInterface
        Handles:
        - AMPQ consumer interface. (handling incoming messages)
        - AMPQ publisher interface (publishing messages)
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass
    
    async def message_handler_callback(self, message_payload : object):
        """
            A method used to route AMQP messages

            Parameters
            ----------

            message_payload : object
                Message object consumed from AMPQ server

            Returns
            ----------
            None
        """
        #from services.segmentation import SegmentationEngine
        from services.segmentation_engine import PastSegmentation, LiveSegmentation, PendingLiveSegmentation
        from data.models.segments import PastSegmentationJob, LiveSegmentationJob
        
        async with message_payload.process(ignore_processed=True):
            routing_key = message_payload.routing_key
            print(f'Routing key :: {routing_key}')
            try:
                message_json = json.loads(message_payload.body.decode())
            except ValueError:
                # if the message_payload is not valid JSON refuse message.
                await message_payload.reject() 
                raise Exception(f'Refused message payload: {message_payload.body.decode()}. Not valid JSON')

            # Switch through routing_key to init desired action(s)
            if routing_key == 'past.segmentation.cmd.run':
                job_data = PastSegmentationJob(**message_json)
                await PastSegmentation(account_id=job_data.account_data.account_id, 
                                    webhook_url=job_data.account_data.webhook_url, 
                                    octy_job_id=job_data.octy_job_id,
                                    segment_id=job_data.segment_data.segment_id).run()

            elif routing_key == 'live.segmentation.cmd.run':
                job_data = LiveSegmentationJob(**message_json)
                
                if job_data.segment_data.segmentation_type == 'live':
                    await LiveSegmentation(account_id=job_data.account_data.account_id,
                                        webhook_url=job_data.account_data.webhook_url, 
                                        octy_job_id=job_data.octy_job_id,
                                        event_obj=job_data.event_data).run()

                elif job_data.segment_data.segmentation_type == 'pending-live':
                    await PendingLiveSegmentation(account_id=job_data.account_data.account_id, 
                                            webhook_url=job_data.account_data.webhook_url, 
                                            octy_job_id=job_data.octy_job_id,
                                            segment_id=job_data.segment_data.segment_id,
                                            profile_id=job_data.event_data.profile.profile_id,
                                            live_octy_job_id=job_data.live_octy_job_id,
                                            event_timeframe=job_data.event_timeframe).run()


    async def publish_message(self, routing_key : str, message_payload : dict):
        """
            A method used to publish AMQP message

            Parameters
            ----------

            routing_key : str
                AMQP routing key for desired exhange

            message_payload : dict
                Message object to send to AMPQ server

            Returns
            ----------
            None
        """
        try:
            p = await _get_set_connection_state('get__publisher_connection')
            await p.send_message(routing_key, message_payload)
        except Exception as e:
            print(e)
            print("No AMQP publisher connection established!")

amqpInterface = AMQPInterface()

class AMQPConsumer():
    """
        _AmqpConsumerRepository
        Handles:
        - Ampq (RabbitMQ) consumser instances
        ...

        Attributes
        ----------
        ...

    """
    EXCHANGE_TYPE = ExchangeType.TOPIC

    def __init__(self, exchange_name : str, callback, logger):
        self.exchange_name = exchange_name
        self.logger = logger
        self.callback = callback
        self.queue_name = ""
        self.routing_key = ""

        self._connection = None
        self._connection_state = 'is_closed' #'connected', 'is_closing', 'is_closed'
        self._forced_closed = False
        self._amqp_url = None
        self._channel = None
        self._exchange = None
        self._consuming = False
        self._reconnect_delay = 0
        self._prefetch_count = 50

    async def connect(self, amqp_url : str)-> None:
        
        self.logger.info(f"Consumer >> Opening conenction to: {amqp_url}")

        recon_wait = 3
        while self._connection_state == 'is_closed':
            try:
                self._connection = await connect_robust(amqp_url, timeout=20)
                self._connection.add_close_callback(callback=self._on_connection_closed)
                self._connection_state = 'connected'
                self.logger.info(f"Consumer >> connection to: {amqp_url} successful!")
                self._forced_closed = False # reset forced closed
                self._amqp_url = amqp_url

            except AMQPException as e:
                self.logger.error(f":: {e}")
                self.logger.warning(f"Consumer >> Conenction to: {amqp_url} failed. Retying in {recon_wait} seconds")

                reconnect_delay = self._get_reconnect_delay()
                if reconnect_delay > 35:
                    exit(1) # Kill application if we can't connect to RabbitMQ
                time.sleep(reconnect_delay)
                    
    def _on_connection_closed(self, _, reason):
        self._connection_state = 'is_closed'
        if not self._forced_closed:
            self.logger.error(f'Consumer >> Connection closed by RabbitMQ. Reason: {reason}. Handling reconnection')
            AMQPStateManager()._reconnect_rabbit(self.logger, self.callback)

    async def _get_reconnect_delay(self) -> int:
        if self._consuming:
            self._reconnect_delay = 0
        else:
            self._reconnect_delay += 1

        if self._reconnect_delay > 30:
            self._reconnect_delay = 30
        elif self._reconnect_delay > 34:
            self._reconnect_delay = 34
        return self._reconnect_delay
                    
    async def close_connection(self) -> None:
        self._forced_closed = True
        self._consuming = False
        if self._connection_state == 'is_closing' or self._connection_state == 'is_closed':
            self.logger.info('Consumer >> Connection is closing or already closed')
        else:
            self.logger.warning(f'Consumer >> Closing connection with {self._amqp_url}')
            await self._connection.close()
            self._connection_state = 'is_closed'
            self.logger.warning('Consumer >> Connection Closed!')

    async def open_channel(self) -> None:
        self._channel = await RobustChannel(self._connection)
        await self._channel.set_qos(self._prefetch_count)
        self.logger.info(f"Consumer >> Opened channel for {self.queue_name}")
        await self.setup_exchange()

    async def setup_exchange(self) -> None: 
        self._exchange = await self._channel.declare_exchange(self.exchange_name, 
                                                            self.EXCHANGE_TYPE, 
                                                            durable=True)
        await self.setup_queue()
        
    async def setup_queue(self) -> None: 
        self._queue = await self._channel.declare_queue(self.queue_name, 
                                                        durable=True, 
                                                        arguments={'x-queue-type': 'quorum'})
        await self.bind_queue()

    async def bind_queue(self) -> None: 
        self.logger.info(f'Consumer >> Binding {self.exchange_name} to queue {self.queue_name} with routing key : {self.routing_key}')
        await self._queue.bind(self._exchange, routing_key=self.routing_key)
        await self.start_consuming()

    async def start_consuming(self) -> None:
        await self._queue.consume(self.callback)
        self._consuming = True
        self.logger.info(f"Consumer >> Consuming on queue {self.queue_name}!")

class AMQPPublisher():
    """
        _AmqpPublisherRepository
        Handles:
        - Ampq (RabbitMQ) publisher instances
        ...

        Attributes
        ----------
        ...
    """

    EXCHANGE_TYPE = ExchangeType.TOPIC

    def __init__(self, exchange_name : str, logger):
        self.exchange_name = exchange_name
        self.logger = logger
        self.queue_name = ""
        self.routing_key = ""

        self._connection = None
        self._connection_state = 'is_closed' #'connected', 'is_closing', 'is_closed'
        self._forced_closed = False
        self._amqp_url = None
        self._channel = None
        self._exchange_map = [] # perisit all initalised channel exchanges
        self._delivery_mode = DeliveryMode.PERSISTENT
        self._reconnect_delay = 0

    async def connect(self, amqp_url : str)-> None:


        self.logger.info(f"Publisher >> Opening conenction to: {amqp_url}")

        recon_wait = 3
        while self._connection_state == 'is_closed':
            try:
                self._connection = await connect_robust(amqp_url, timeout=20)
                self._connection.add_close_callback(callback=self._on_connection_closed)
                self._connection_state = 'connected'
                self.logger.info(f"Publisher >>  connection to: {amqp_url} successful!")
                self._forced_closed = False # reset forced closed
                self._amqp_url = amqp_url
            except AMQPException as e:
                self.logger.error(f":: {e}")
                self.logger.warning(f"Publisher >>  Conenction to: {amqp_url} failed. Retying in {recon_wait} seconds")

                reconnect_delay = self._get_reconnect_delay()
                if reconnect_delay > 35:
                    exit(1) # Kill application if we can't connect to RabbitMQ
                time.sleep(reconnect_delay)

    def _on_connection_closed(self, _, reason):
        self._connection_state = 'is_closed'
        if not self._forced_closed:
            self.logger.error(f'Publisher >> Connection closed by RabbitMQ. Reason: {reason}. Handling reconnection')
            AMQPStateManager()._reconnect_rabbit(self.logger, None)

    async def _get_reconnect_delay(self):
        self._reconnect_delay += 1
        if self._reconnect_delay > 30:
            self._reconnect_delay = 30
        elif self._reconnect_delay > 34:
            self._reconnect_delay = 34
        return self._reconnect_delay
                    
    async def close_connection(self) -> None:
        self._forced_closed = True
        if self._connection_state == 'is_closing' or self._connection_state == 'is_closed':
            self.logger.info('Publisher >> Connection is closing or already closed')
        else:
            self.logger.warning(f'Publisher >> Closing connection with {self._amqp_url}')
            await self._connection.close()
            self._connection_state = 'is_closed'
            self.logger.warning('Publisher >> Connection Closed!')

    async def open_channel(self) -> None:
        self._channel = await RobustChannel(self._connection)
        self.logger.info(f"Publisher >>  Opened channel for {self.queue_name}")

        await self.setup_exchange()

    async def setup_exchange(self) -> None: 
        exchange = await self._channel.declare_exchange(self.exchange_name, 
                                                            self.EXCHANGE_TYPE, 
                                                            durable=True)
        self._exchange_map.append({
            'exchange_name': self.exchange_name,
            'channel':  self._channel,
            'exchange': exchange,
            'routing_key' : self.routing_key
        })
        
    async def send_message(self, routing_key : str, payload : dict) -> bool:

        exchange =  next((e for e in self._exchange_map if e["routing_key"] == routing_key), None)

        message = Message(
            json.dumps(payload).encode('utf-8'),
            delivery_mode=self._delivery_mode
        )

        try:
            # Sending the message
            await exchange['exchange'].publish(message, routing_key=routing_key)
        except PublishError as e:
            self.logger.error(f'{e}')
            return False
            
        self.logger.info(f"Publisher >> [x] Sent {message}")

        return True

class AMQPStateManager():
    """
        AMQPStateManager
        Handles:
        - Initialising AMPQ desired connection(s) state
        - Persisting AMPQ desired connection(s) state
        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass


    async def init_consumers(self, logger):

        try:
            Config['AMQP_CONSUMERS']
        except KeyError:
            logger.warning("No consumer instances configured for this service")
        else:
            #Open single long lived connection for all consumers
            consumer_conn = AMQPConsumer(exchange_name=Config['EXCHANGE'], 
                                            callback=amqpInterface.message_handler_callback, 
                                            logger=logger)
            await consumer_conn.connect(Config['AMQP_URL'])
            await _get_set_connection_state('set__consumer_connection', consumer_connection=consumer_conn)

            # Open channels for each consumer
            for consumer in Config['AMQP_CONSUMERS']:
                consumer_conn.queue_name = consumer['QUEUE']
                consumer_conn.routing_key = consumer['ROUTING_KEY']
                await asyncio.create_task(consumer_conn.open_channel())

    async def init_publishers(self, logger):
        try:
            Config['AMQP_PUBLISHERS']
        except KeyError:
            logger.warning("No publisher instances configured for this service")
        else:

            #Open single long lived connection for all publishers
            publisher_conn = AMQPPublisher(exchange_name=Config['EXCHANGE'],logger=logger)
            await publisher_conn.connect(Config['AMQP_URL'])

            # update global publisher connection object
            await _get_set_connection_state('set__publisher_connection', publisher_connection=publisher_conn)

            # Open channels for each publisher
            for publisher in Config['AMQP_PUBLISHERS']:
                publisher_conn.queue_name = publisher['QUEUE']
                publisher_conn.routing_key = publisher['ROUTING_KEY']
                await asyncio.create_task(publisher_conn.open_channel())
  

    async def _reset_state(self, consumer_conn, publisher_conn, logger):

        # Kill any existing open connections
        c,p = await _get_set_connection_state('get__all')
        if c:
            await c.close_connection()
        if p:
            await p.close_connection()

        try:
            Config['AMQP_CONSUMERS']
        except KeyError:
            logger.warning("No consumer instances configured for this service")
        else:
            # CONSUMERS
            await consumer_conn.connect(Config['AMQP_URL'])
            await _get_set_connection_state('set__consumer_connection', consumer_connection=consumer_conn)

            # Open channels for each consumer
            for consumer in Config['AMQP_CONSUMERS']:
                consumer_conn.queue_name = consumer['QUEUE']
                consumer_conn.routing_key = consumer['ROUTING_KEY']
                await consumer_conn.open_channel()

        try:
            Config['AMQP_PUBLISHERS']
        except KeyError:
            logger.warning("No publisher instances configured for this service")
        else:

            # PUBLISHERS
            await publisher_conn.connect(Config['AMQP_URL'])
            await _get_set_connection_state('set__publisher_connection', publisher_connection=publisher_conn)

            # Open channels for each publisher
            for publisher in Config['AMQP_PUBLISHERS']:
                publisher_conn.queue_name = publisher['QUEUE']
                publisher_conn.routing_key = publisher['ROUTING_KEY']
                await asyncio.create_task(publisher_conn.open_channel())
    

        logger.info("RESET RABBIT STATE")

    def _reconnect_rabbit(self, logger, callback):
        logger.warning("Resetting Rabbit state")
        #Open single long lived connection for all consumers
        consumer_conn = AMQPConsumer(exchange_name=Config['EXCHANGE'], 
                                        callback=callback, 
                                        logger=logger)
        publisher_conn = AMQPPublisher(exchange_name=Config['EXCHANGE'],logger=logger)

        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:  # if cleanup: 'RuntimeError: There is no current event loop..'
            loop = None
        if loop and loop.is_running():
            loop.create_task(self._reset_state(consumer_conn, publisher_conn, logger))
        else:
            logger.info('Starting new event loop')
            loop.create_task(self._reset_state(consumer_conn, publisher_conn, logger))


async def _get_set_connection_state(action : str, 
                            consumer_connection : object = None, 
                            publisher_connection : object = None):
    from worker import app

    if action == 'set__consumer_connection':
        app.state.consumer_connection = consumer_connection
        return consumer_connection
    elif action == 'set__publisher_connection':
        app.state.publisher_connection = publisher_connection
        return publisher_connection
    elif action == 'get__consumer_connection':
        return app.state.consumer_connection
    elif action == 'get__publisher_connection':
        return app.state.publisher_connection
    elif action == 'get__all':
        try:
            consumer_connection = app.state.consumer_connection
        except AttributeError:
             consumer_connection = None
        try:
            publisher_connection = app.state.publisher_connection
        except AttributeError:
             publisher_connection = None
        return consumer_connection, publisher_connection