# module imports
from data.repositories.Iitems_repository import ItemsInterface
from data.models.db_schemas import tbl_items
from api.routers.request_models.items import *
from utils.utils import *
from api.routers.error_handlers import *
from services.AMQP import amqpInterface


# python imports
from typing import *
import json
from datetime import datetime as dt
import copy

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from pymongo.errors import BulkWriteError



class _ItemsRepository(ItemsInterface):
    """
        _ItemsRepository
        Handles:
        - Retrieving items
        - Creating items
        - Updating items
        - Deleting items

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    def get_item_count(self, account_id : str) -> int:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        return tbl_items.objects(account_id__exact=account_id).count()

    def get_item_by_id(self, item_id : str, account_id : str) -> dict:
        """
        Parameters
        ----------
        item_id : str
            The item_id of the item that should be returned.
        account_id : str
            Octy account id

        Returns
        ----------
        results : dict
        """
        items = tbl_items.objects((Q(item_id__exact=item_id) & Q(account_id__exact=account_id)))
        if items:
            item_dict = json.loads(items.to_json())
            #item_dict[0]['item_id'] = item_dict[0]['_id']
            item_dict= _format_item(item_dict[0])
            return item_dict
        return None

    def get_items(self,
                  account_id : str,
                  cursor : int = None,
                  ids : bool = False,
                  status : str = 'all') -> Union[list, int]:
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
        results : dict
        total : int
        """
        item_dicts=[]
        total=0
        if status == 'all':
            if ids: 
                items = tbl_items.objects.only('item_id')(account_id__exact=account_id).skip(cursor).limit(200)
            else:
                items = tbl_items.objects(account_id__exact=account_id).skip(cursor).limit(100)
            if items:
                total += tbl_items.objects(account_id__exact=account_id).count()
                item_dicts = json.loads(items.to_json())
        else:
            if ids: 
                items = tbl_items.objects.only('item_id')(account_id__exact=account_id, status__exact=status).skip(cursor).limit(200)
            else:
                items = tbl_items.objects(account_id__exact=account_id, status__exact=status).skip(cursor).limit(100)
            if items:
                total += tbl_items.objects(account_id__exact=account_id, status__exact=status).count()
                item_dicts = json.loads(items.to_json())
        
        #format items
        for item in item_dicts:
            #item['item_id'] = item['_id']
            _format_item(item)
        return item_dicts, total

    def create_items(self, items_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        items_batch : List
            list of item object dictonaries (valid item objects)

        Returns
        ----------
        created_items, failed_to_create items
        """
        item_instances = []
        item_ids = []
        for item in items_batch:
            item_instances.append(
                tbl_items(
                    item_id=item['item_id'],
                    account_id=item['account_id'],
                    item_category=item['item_category'],
                    item_name=item['item_name'],
                    item_description=item['item_description'],
                    item_price=item['item_price'],
                    event_type=item['event_type']
                )
            )
            item_ids.append(item['item_id'])

        #BULK WRITE OPERATION
        invalid=[]
        bulk_operation = tbl_items._get_collection().initialize_unordered_bulk_op()
        for item in item_instances:
            bulk_operation.insert(item.to_mongo())
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                invalid.append(err['op'].to_dict()['item_id'])

        valid = list(set(item_ids) - set(invalid))

        failed_to_create=[]
        for in_ in invalid:
            failed_to_create.append(
                {
                    'item_id': in_,
                    'error_message' : f'Another item exists with provided item_id : {in_}'
                }
            )
        created_items=[]
        for v in valid:
            item=next((d for i,d in enumerate(items_batch) if v == d['item_id']),None)
            if item:
                item.pop('account_id', None)
                item.pop('event_type', None)
                created_items.append(item)
        
        return created_items, failed_to_create

    def update_items(self, items_batch : list, account_id : str) -> Union[list, list]:
        """
        Parameters
        ----------
        items_batch : List
            list of item object dictonaries (valid item objects)
        account_id : str
            Octy account id

        Returns
        ----------
        updated items : list
        not found / invalid items: list
        """
        updated_items = []
        failed_to_update=[]
        not_existing_items = []
        item_ids = [] # provided items ids array

        # determine valid items
        for item in items_batch:
            if item['item_id'] in item_ids:
                raise OctyException(400,'An error occurred when validating request.', [{'message' : f'Identical item identifers supplied. Found duplicate item_id : {item["item_id"]}', 
                'extended_help': Config['ITEMS_EXTENDED_HELP']}])
            item_ids.append(item['item_id'])

        items = json.loads(tbl_items.objects(item_id__in=item_ids, account_id__exact=account_id).to_json())
        if not items:
            for item in items_batch:
                failed_to_update.append(
                    {
                        'item_id' : item['item_id'],
                        'error_message' : f'No item found with item_id : {item["item_id"]}'
                    }
                )
            return updated_items, failed_to_update
     
        for itd in item_ids:
            exists=next((key for key in items if key['item_id'] == itd), None)
            if not exists:
                item_batch_obj=next((key for key in items_batch if key['item_id'] == itd), None)
                not_existing_items.append(item_batch_obj['item_id'])
                failed_to_update.append(
                    {
                        'item_id': itd,
                        'error_message' : f'No item exists with provided item_id : {itd}'
                    }
                )

        #BULK UPDATE OPERATION
        bulk_operation = tbl_items._get_collection().initialize_unordered_bulk_op()
        for i in items:
            item_batch_obj = next(key for key in items_batch if key['item_id'] == i['item_id'])

            # build update dict
            set_dict = DictConditional(lambda x: x != None)
            set_dict['item_id'] = i['item_id']
            set_dict['item_category'] = item_batch_obj['item_category']
            set_dict['item_name'] = item_batch_obj['item_name']
            set_dict['item_description'] = item_batch_obj['item_description']
            set_dict['item_price'] = item_batch_obj['item_price']
            set_dict['event_type'] = item_batch_obj['event_type']
            set_dict['status'] = item_batch_obj['status']
            set_dict['updated_at'] = dt.now()

            bulk_operation.find({
                '$and' : [
                    {"item_id" : { "$eq" : i['item_id']}},
                    {"account_id" : { "$eq" : i['account_id']}}
                ]
            }).update(
                {
                    "$set" : set_dict
                }
            )

            # append updated item to return array
            item_batch_obj['created_at'] = i['created_at']
            item_batch_obj['updated_at'] = dt.now()
            updated_items.append(_format_item(item_batch_obj))
        
        try:
            bulk_operation.execute()
        except BulkWriteError as bwe:
            for err in bwe.details['writeErrors']:
                if err['code'] == 11000:
                    mes = f"Another item exists with provided item_id : {err['op']['u']['$set']['item_id']}"
                else:
                    mes = f"Unknown error occurred when updating item with item_id : {err['op']['u']['$set']['item_id']}"
                failed_to_update.append({
                        'item_id' : err['op']['u']['$set']['item_id'],
                        'error_message' : mes
                    })


                updated_items = list(filter(lambda i : i['item_id'] != err['op']['u']['$set']['item_id'], updated_items))


        return updated_items, failed_to_update

    async def delete_items(self, items_batch : list, account : object) -> Union[list, list]:
        """
        Parameters
        ----------
        items_batch : List
            list of item object dictonaries (valid item objects)
        account : Octy account

        Returns
        ----------
        deleted_items : list
        failed_to_delete : list
        """
        deleted_items=[]
        failed_to_delete=[]
        item_ids=[]

        for item in items_batch:
            item_ids.append(item['item_id'])


        items = json.loads(tbl_items.objects(item_id__in=item_ids, account_id__exact=account.account_id).to_json())
        if not items:
            for item in items_batch:
                failed_to_delete.append(
                    {
                        'item_id' : item['item_id'],
                        'error_message' : f'No item found with item_id : {item["item_id"]}'
                    }
                )
            return deleted_items, failed_to_delete

        
        
        bulk_operation = tbl_items._get_collection().initialize_unordered_bulk_op()
        for item in items_batch:
            item_batch_object=next((key for key in items if key['item_id'] == item['item_id'] and key['account_id'] == item['account_id']), None)
            if item_batch_object:
                deleted_items.append(
                    {
                        'item_id': item_batch_object['item_id']
                    }
                )
            else:
                failed_to_delete.append(
                    {
                        'item_id' : item['item_id'],
                        'error_message' : f'No item found with item_id : {item["item_id"]}'
                    }
                )

            bulk_operation.find({
                '$and' : [
                    {  "item_id" : { "$eq" : item['item_id'] }  },
                    {  "account_id" : { "$eq" : item['account_id'] }  }
                ]
            }).remove()

        bulk_operation.execute()

        # update item_id_stop_list in account configurations
        rec_configs = next((key for key in account.algorithm_configurations if key['algorithm_name'] == 'rec'), None)
        if rec_configs:
            try:
                item_id_stop_list=rec_configs['config_json']['item_id_stop_list']
            except KeyError:
                item_id_stop_list=[]
                
            augmented_stop_list=copy.deepcopy(item_id_stop_list)
            for item in deleted_items:
                # iterate over item_id_stop_list, if this in item_id_stop_list augment item_id_stop_list
                for sl_item in item_id_stop_list:
                    if sl_item['item_id'] == item['item_id']:
                        index_= next((index for (index, d) in enumerate(augmented_stop_list) \
                            if d['item_id'] == sl_item['item_id']), None)
                        del augmented_stop_list[index_]
            #populate new stop list from augmented list
            new_stop_list = list()
            for i in augmented_stop_list:
                new_stop_list.append(
                    i
                )
                  
            # publish message to update item_id_stop_list in account configurations
            rec_configs['config_json']['item_id_stop_list'] = new_stop_list
            print(rec_configs['config_json'])
            await amqpInterface.publish_message(routing_key='algo.configs.cmd.update',
                message_payload={
                    "account_id" : account.account_id,
                    "algorithm_configurations" : {
                        "algorithm_name" : 'rec',
                        "config_json" : rec_configs['config_json']
                    }
                
                })

        return deleted_items, failed_to_delete


itemsRepository = _ItemsRepository()


def _format_item(item : dict):
    '''
        Format item objects
    '''
    
    item.pop('_id', None)
    item.pop('account_id', None)
    item.pop('event_type', None)
    try:
        item['created_at'] = int_to_dt(item['created_at']['$date'], as_str=True) if item['created_at'] != None else None
        try:
            item['updated_at'] = int_to_dt(item['updated_at']['$date'], as_str=True) if item['updated_at'] != None else None
        except TypeError:
            item['updated_at'] = item['updated_at'].strftime('%a, %d %b %Y %H:%M:%S GMT')
    except:
        return item

    return item