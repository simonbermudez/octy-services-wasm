# module imports

# python imports
import sys
import time

# external imports
import numpy as np
from octy_rabbitmq.amqp_publisher import amqpPublisher


class BillingUnits():

    def __init__(self, account_id, account_type, account_currency, process_name):
        self.account_id = account_id
        self.account_type = account_type
        self.account_currency = account_currency
        self.process_name = process_name
        #compute
        self.compute_quantity = 0
        self.compute_metric='hours'
        self.capturuing_compute_units=False
        #data
        self.data_quantity = 0
        self.data_metric='KB'
        self.capturuing_data_units=False

    # compute
    async def track_compute_units(self, metric):
        if metric not in ['seconds', 'minutes', 'hours']:
            raise Exception(f"Unknown compute metric specified: {metric}")
        self.compute_start_time = time.time()
        self.compute_metric=metric
        self.capturuing_compute_units=True
    
    async def complete_compute_units(self, additional_unit_hours=0):
        complete_time = time.time()
        if self.compute_metric == 'seconds':
            self.compute_quantity = int(np.ceil((complete_time - self.compute_start_time))) + int(np.ceil((additional_unit_hours/60)/60))
        elif self.compute_metric == 'minutes':
            self.compute_quantity = int(np.ceil(((complete_time - self.compute_start_time)/60))) + int(np.ceil(additional_unit_hours/60))
        elif self.compute_metric == 'hours':
            self.compute_quantity = int(np.ceil((((complete_time - self.compute_start_time)/60)/60))) + int(np.ceil(additional_unit_hours))
        await self._capture_units('compute')

    # data
    async def track_data_units(self, unit):
        self.data_quantity += _get_size(unit)
        self.capturuing_data_units=True

    async def complete_data_units(self, metric):
        self.data_metric=metric
        self.data_quantity = _bytes_to_metric(self.data_quantity, self.data_metric)
        await self._capture_units('data')


    async def _capture_units(self, unit_type):
        if self.capturuing_data_units and unit_type == 'data':
            self.capturuing_data_units=False
            await amqpPublisher.send_message(
                routing_key='account.billing.cmd.capture',
                payload={
                    'units' : [
                        {
                            'unit_type' : unit_type, 
                            'metric' : self.data_metric,
                            'process_name' : self.process_name,
                            'quantity' : self.data_quantity if self.data_quantity > 0 else 1,
                            'account_id' : self.account_id,
                            'account_currency' : self.account_currency,
                            'account_type' : self.account_type
                        }
                    ]
                    

                }
            )
        if self.capturuing_compute_units and unit_type == 'compute':
            self.capturuing_compute_units=False
            await amqpPublisher.send_message(
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


# helpers
def _bytes_to_metric(bytes, metric) -> int:
    if metric == 'KB':
        return round(bytes / 1000)
    elif metric == 'MB':
        return round(bytes / 1000000)
    elif metric == 'GB':
        return round(bytes / 1000000000)
    elif metric == 'TB':
        return round(bytes / 1000000000000)
    else:
        raise Exception(f"Unknown data metric specified: {metric}")

def _get_size(obj, seen=None):
    """Recursively finds size of objects"""
    size = sys.getsizeof(obj)
    if seen is None:
        seen = set()
    obj_id = id(obj)
    if obj_id in seen:
        return 0
    # Important mark as seen *before* entering recursion to gracefully handle
    # self-referential objects
    seen.add(obj_id)
    if isinstance(obj, dict):
        size += sum([_get_size(v, seen) for v in obj.values()])
        size += sum([_get_size(k, seen) for k in obj.keys()])
    elif hasattr(obj, '__dict__'):
        size += _get_size(obj.__dict__, seen)
    elif hasattr(obj, '__iter__') and not isinstance(obj, (str, bytes, bytearray)):
        size += sum([_get_size(i, seen) for i in obj])
    return size