# module imports
from data.repositories.implementation.billing_repository import billingRepository
from data.models.billing import *
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
from datetime import datetime as dt

# external imports


class BillingService():
    """
        BillingService
        Handles:
        - Get billable untis
        - Calculate & persist billable untis
        ...

        Attributes
        ----------
        None
    """
    def __init__(self): pass

    async def calculate_persist_billable_units(billableUnits : BillableUnits) -> None :
        units = []
        for unit in billableUnits:
            unit_type=next((u for u in Config['UNITS'] if u['unit_type'] == unit.unit_type), None)
            if not unit_type:
                raise Exception(f"[toxic]:: Unknown unit type provided : {unit.unit_type}")

            metric=next((m for m in unit_type['metrics'] if m['name'] == unit.metric), None)
            if not metric:
                raise Exception(f"[toxic]:: Unknown metric provided : {unit.metric} for unit type: {unit.unit_type}")

            try:
                costs = metric['costs'][unit.account_currency]
                currency = unit.account_currency
            except KeyError:
                # Default to GBP
                costs = metric['costs']['GBP']
                currency = 'GBP'

            try:
                fee = costs[unit.account_type]
            except KeyError:
                raise Exception(f"[toxic]:: Unknown account type provided : {unit.account_type}")
            
            units.append(
                {
                    'account_id' : unit.account_id,
                    'account_type' : unit.account_type,
                    'process_name' : unit.process_name,
                    'unit_type' : unit.unit_type,
                    'metric' : unit.metric,
                    'quantity' : unit.quantity,
                    'cost_per_unit' : fee,
                    'total_cost' : fee * unit.quantity,
                    'currency' : currency
                }
            )

        await billingRepository.create_billable_units_ref(units)
    
    async def get_billable_units(self, 
                                account_ids : list = None, 
                                account_types : list = None, 
                                unit_types : list = None, 
                                metrics : list = None,
                                process_names : list = None,
                                cost_upper_range : int = None, 
                                cost_lower_range : int = None,
                                currencies : list = None,
                                created_at_upper_range : str = None,
                                created_at_lower_range : str = None,
                                cursor : int = 0) -> Union[list, int]:
        filters = {}
        if account_ids: 
            filters['account_ids'] = account_ids
        if account_types:
            filters['account_types'] = account_types
        if unit_types:
            filters['unit_types'] = unit_types
        if metrics:
            filters['metrics'] = metrics
        if process_names:
            filters['process_names'] = process_names
        if cost_upper_range:
            filters['cost_upper_range'] = cost_upper_range
        if cost_lower_range:
            filters['cost_lower_range'] = cost_lower_range
        if currencies:
            filters['currencies'] = currencies
        if created_at_upper_range:
            filters['created_at_upper_range'] = dt.strptime(created_at_upper_range, '%Y-%m-%d')
        if created_at_lower_range:
            filters['created_at_lower_range'] = dt.strptime(created_at_lower_range, '%Y-%m-%d')

        return await billingRepository.filter_billable_units(filters, cursor)

    # delete all billable units for an account
    async def delete_account_billing_internal(self, account_id : str) -> bool:
        return await billingRepository.delete_account_billing_internal(account_id)
    
