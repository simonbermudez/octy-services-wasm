# module imports
from data.repositories.implementation.items_repository import itemsRepository
from api.routers.request_models.items import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config
from .billing import BillingUnits

# python imports
from typing import *
import json

# external imports
from fastapi import Request


class ItemsService():
    """
        ItemsService
        Handles:
        - Get Items
        - Items creation
        - Update Items
        - Delete Items
        ...

        Attributes
        ----------
        account : Octy account
    """
    def __init__(self, account : Account, account_id : str = None): 
        # self.account = account
        # self.account_id = account_id if account_id != None else account.account_id
        # self.b = None if self.account is None else BillingUnits(account_id=self.account.account_id, account_type=self.account.account_configurations['a_t'], account_currency=self.account.account_configurations['a_c'], process_name='items_data')
        self.account = account
        if account_id is not None:
            self.account_id = account_id
        elif account is not None:
            self.account_id = account.account_id
    
        if self.account is None:
           self.b = None 
        else:
           self.b = BillingUnits(
            account_id=self.account.account_id, 
            account_type=self.account.account_configurations['a_t'], 
            account_currency=self.account.account_configurations['a_c'], 
            process_name='items_data'
        )

    def get_items(self,
                  item_ids : list = None, 
                  cursor : int = None) -> Union[dict, int]:
        """
        Parameters
        ----------
        item_ids : list
            list of item_ids
        cursor : int
            Pagination cursor

        Returns
        ----------
        items : dict
        total : int
        """
        if item_ids != None and cursor == 0:
            items = itemsRepository.get_item_by_ids(item_ids=item_ids,account_id=self.account.account_id)
            count = len(items)
            if count<1:
                raise OctyException(400, 'Invalid item identifier(s) provided', 
                [{'error_message' : 'No items were found with the provided identifier(s)', 
                'extended_help': Config['ITEMS_EXTENDED_HELP']}])
            
            return items, count
            

        elif item_ids == None and cursor != None:
            
            items,total = itemsRepository.get_items(account_id=self.account.account_id, 
                                                cursor=cursor)
            if len(items)<1:
                raise OctyException(400, 'No items found', 
                [{'error_message' : 'No items found with the provided item identifier or pagination cursor exhausted', 
                'extended_help': Config['ITEMS_EXTENDED_HELP']}])
            return items, total

    async def create_items(self, items : CreateItems) -> Union[list, list]:
        """
        Parameters
        ----------
        items : CreateItems
            CreateItems request model instance

        Returns
        ----------
        Created and failed to create items : Union[list, list]
        """

        # assess allowed limits
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              itemsRepository.get_item_count(self.account.account_id),
                              len(items.items))
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'error_message' : f'This request could not be completed as the number of items sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} items.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        items_batch = []
        for item in items.items:
            items_batch.append(
                {
                    'item_id' : item.item_id,
                    'account_id' : self.account.account_id,
                    'item_category' : item.item_category,
                    'item_name' : item.item_name,
                    'item_description' : item.item_description,
                    'item_price' : item.item_price,
                    'event_type' : 'charged'
                }
            )

        created, failed = itemsRepository.create_items(items_batch)

        if len(created) < 1:
            raise OctyException(400, 'No items created!', failed)

        await self.b.track_data_units(created)
        await self.b.complete_data_units('MB')

        return created, failed
    
    async def update_items(self, items : UpdateItems) -> Union[list, list]:
        """
        Parameters
        ----------
        items : UpdateItems
            UpdateItems request model instance

        Returns
        ----------
        Updated and failed to update items : Union[list, list]
        """
        items_batch = []
        for item in items.items:
            items_batch.append(
                {
                    'item_id' : item.item_id,
                    'account_id' : self.account.account_id,
                    'item_category' : item.item_category,
                    'item_name' : item.item_name,
                    'item_description' : item.item_description,
                    'item_price' : item.item_price,
                    'status' : item.status,
                    'event_type' : 'charged'
                }
            )

        updated, failed = itemsRepository.update_items(items_batch, self.account.account_id)

        if len(updated) < 1:
            raise OctyException(400, 'No items updated!', failed)

        await self.b.track_data_units(updated)
        await self.b.complete_data_units('MB')

        return updated, failed

    async def delete_items(self, items : DeleteItems) -> Union[list, list]:
        """
        Parameters
        ----------
        items : DeleteItems
            DeleteItems request model instance
    
        Returns
        ----------
        Deleted and failed to delete item ids : Union[list, list]
        """
        items_batch = []
        for item in items.items:
            items_batch.append({
                "item_id" : item,
                "account_id" : self.account.account_id
            })

        deleted , failed = await itemsRepository.delete_items(items_batch, self.account)

        if len(deleted) < 1:
            raise OctyException(400, 'No items deleted!', failed)
        return deleted, failed


    #INTERNAL

    def get_items_internal(self, account_id : str, cursor : int, ids : bool, status : str) -> Union[dict, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        cursor : int
            Pagination cursor
        ids : bool
            Only get item id(s)
        status : str

        Returns
        ----------
        items : dict
        total : int
        """
        items,total = itemsRepository.get_items(account_id=account_id, cursor=cursor, ids=ids, status=status)
        if len(items)<1:
            raise OctyException(400, 'No items found', 
            [{'error_message' : 'No items found or pagination cursor exhausted', 
            'extended_help': Config['ITEMS_EXTENDED_HELP']}])
        return items, total
    
    #Delete all items for an account
    async def delete_account_items_internal(self, account_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        bool
        """
        res = await itemsRepository.delete_account_items_internal(account_id=account_id)
        return res