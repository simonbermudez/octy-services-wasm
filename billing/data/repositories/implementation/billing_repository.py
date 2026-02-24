# module imports
from time import process_time
from data.models.db_schemas import tbl_billable_units
from data.repositories.Ibilling_repository import BillingInterface
from utils.utils import *


# python imports
from typing import *
import json

# external imports
from bson.json_util import dumps


class _BillingRepository(BillingInterface):
    """
        _BillingRepository
        Handles:
        - Persisting billable units
        - filtering and returning billable units

        ...

        Attributes
        ----------
        None
    """
    def __init__(self): pass

    async def create_billable_units_ref(self, untis : list) -> None:
        """
        Parameters
        ----------
        untis : list
            List of billable unit objects

        Returns
        ----------
        None
        """
        #BULK WRITE OPERATION
        bulk_operation = tbl_billable_units._get_collection().initialize_unordered_bulk_op()
        for unit in untis:
            bulk_operation.insert(
                tbl_billable_units(
                    account_id=unit['account_id'],
                    account_type=unit['account_type'],
                    process_name=unit['process_name'],
                    unit_type=unit['unit_type'],
                    metric=unit['metric'],
                    quantity=unit['quantity'],
                    cost_per_unit=unit['cost_per_unit'],
                    total_cost=unit['total_cost'],
                    currency=unit['currency']
                ).to_mongo()
            )
        try:
            bulk_operation.execute()
        except Exception as ex:
            raise Exception(f"Failed to create billable units reference. Exception: {ex}")

    async def filter_billable_units(self, filters : dict, cursor : int) -> Union[list, int]:
        """
        Parameters
        ----------
        filters : dict
            Specific filter parameters
            example: 
            {
                'account_ids' : ['account-123'], 
                'account_types' : ['pro', 'startup', 'enterprise'],
                'unit_types' : ['data','compute'],
                'metrics' : ['MB'],
                'cost_upper_range' : 300,
                'cost_lower_range' : 10,
                'currencies' : ['GBP', 'USD'],
                'created_at_upper_range' : {date-time},
                'created_at_lower_range' : {date-time},
                'process_names' : ['rec-training', 'past-segmentation']
            }
        cursor : int
            Pagination cursor

        Returns
        ----------
        billable units : list
        """
        def _filter_exists(obj, key):
            try:
                return obj[key]
            except KeyError:
                return None

        queries = []

        # build query based on filter parameters

        if _filter_exists(filters, 'account_ids'):
            queries.append(
                {"account_id" : {"$in" : filters['account_ids']}}
            )
        if _filter_exists(filters, 'account_types'):
            queries.append(
                {"account_type" : {"$in" : filters['account_types']}}
            )
        if _filter_exists(filters, 'unit_types'):
            queries.append(
                {"unit_type" : {"$in" : filters['unit_types']}}
            )
        if _filter_exists(filters, 'metrics'):
            queries.append(
                {"metric" : {"$in" : filters['metrics']}}
            )
        if _filter_exists(filters, 'process_names'):
            queries.append(
                {"process_name" : {"$in" : filters['process_names']}}
            )
        if _filter_exists(filters, 'cost_upper_range'):
            queries.append(
                {"total_cost" : {"$lte" : filters['cost_upper_range']}}
            )
        if _filter_exists(filters, 'cost_lower_range'):
            queries.append(
                {"total_cost" : {"$gte" : filters['cost_lower_range']}}
            )
        if _filter_exists(filters, 'currencies'):
            queries.append(
                {"currency" : {"$in" : filters['currencies']}}
            )
        if _filter_exists(filters, 'created_at_upper_range'):
            queries.append(
                {"created_at" : {"$lte" : filters['created_at_upper_range']}}
            )
        if _filter_exists(filters, 'created_at_lower_range'):
            queries.append(
                {"created_at" : {"$gte" : filters['created_at_lower_range']}}
            )

        query = {'$and' : queries} if len(queries) > 0 else None

        results_cursor = tbl_billable_units._get_collection().find(query).skip(cursor).limit(2000)
        total = tbl_billable_units._get_collection().find(query).count()
        raw_units = json.loads(dumps(list(results_cursor), indent = 2))

        for unit in raw_units:
            unit["created_at"] = int_to_dt(unit['created_at']['$date'], as_str=True) if unit['created_at'] != None else None
        
        return raw_units, total
    
    # Delete billable units to do with account_id
    async def delete_account_billable_units(self, account_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        bool
        """
        try:
            tbl_billable_units.objects(account_id=account_id).delete()
            return True
        except Exception as ex:
            raise Exception(f"Failed to delete billable units. Exception: {ex}")


billingRepository = _BillingRepository()