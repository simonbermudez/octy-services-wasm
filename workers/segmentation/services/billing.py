# module imports

# python imports
import time

# external imports
import numpy as np
from octy_rabbitmq.amqp_publisher import amqpPublisher


class BillingUnits():

    def __init__(self, account_id, account_type, account_currency, process_name, loop):
        self.account_id = account_id
        self.account_type = account_type
        self.account_currency = account_currency
        self.process_name = process_name
        self.loop = loop
        self.compute_quantity = 0
        self.compute_metric='hours'
        self.capturuing_compute_units=False

    def track_compute_units(self, metric):
        if metric not in ['seconds', 'minutes', 'hours']:
            raise Exception(f"Unknown compute metric specified: {metric}")
        self.compute_start_time = time.time()
        self.compute_metric=metric
        self.capturuing_compute_units=True
    
    def complete_compute_units(self, additional_unit_hours=0):
        complete_time = time.time()
        if self.compute_metric == 'seconds':
            self.compute_quantity = int(np.ceil((complete_time - self.compute_start_time))) + int(np.ceil((additional_unit_hours/60)/60))
        elif self.compute_metric == 'minutes':
            self.compute_quantity = int(np.ceil(((complete_time - self.compute_start_time)/60))) + int(np.ceil(additional_unit_hours/60))
        elif self.compute_metric == 'hours':
            self.compute_quantity = int(np.ceil((((complete_time - self.compute_start_time)/60)/60))) + int(np.ceil(additional_unit_hours))
        self._capture_units('compute')

    def _capture_units(self, unit_type):
        self.capturuing_compute_units=False
        self.loop.create_task(
            amqpPublisher.send_message(
                routing_key='account.billing.cmd.capture',
                payload={
                    'units' : [
                        {
                            'unit_type' : unit_type, 
                            'metric' : self.compute_metric,
                            'process_name' : self.process_name,
                            'quantity' : self.compute_quantity if self.compute_quantity > 0 else 1,
                            'account_id' : self.account_id,
                            'account_currency' : self.account_currency,
                            'account_type' : self.account_type
                        }
                    ]
                    

                }
            )
        )