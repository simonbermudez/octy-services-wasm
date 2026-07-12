# module imports
from data.repositories.Itemplates_repository import TemplatesInterface
from data.models.db_schemas import tbl_templates
from utils.utils import *
from api.routers.error_handlers import *


# python imports
from typing import *
import json
from datetime import datetime as dt

# external imports
from mongoengine.errors import BulkWriteError
from mongoengine.queryset.visitor import Q
from bson.json_util import dumps


class _TemplatesRepository(TemplatesInterface):
    """
        _TemplatesRepository
        Handles:
        - Retrieving templates
        - Creating templates
        - Updating templates
        - Deleting templates

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass

    async def get_all_templates(self, account_id : str) -> list:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        templates : list
        """
        return tbl_templates.objects(account_id__exact=account_id, status__exact='active')

    def get_template_count(self, account_id : str) -> int:
        """
        Parameters
        ----------
        account_id : str
            Octy account id

        Returns
        ----------
        count : int
        """
        return tbl_templates.objects(account_id__exact=account_id, status__exact='active').count()

    async def get_templates(self, account_id : str, identifiers : list = None, cursor : int = 0) -> Union[list, int]:
        """
        Parameters
        ----------
        account_id : str
            Octy account id
        identifiers : list
        cursor : int

        Returns
        ----------
        templates : list
        total : int
        """
        query = [
            {"account_id" : { "$eq" : account_id}},
            {"status" : { "$eq" : "active"}}
        ]

        if identifiers != None:
            cursor = 0
            query.append(

                {
                    "$or" : [
                        {"_id" : { "$in" : identifiers}},
                        {"friendly_name" : { "$in" : identifiers}},
                        {"template_type" : { "$in" : identifiers}}
                    ]
                    
                }
            
            )

        results_cursor = tbl_templates._get_collection().find({'$and' : query}).skip(cursor).limit(100)
        total = tbl_templates._get_collection().find({'$and' : query}).count()
        raw_res = json.loads(dumps(list(results_cursor), indent = 2))
        
        #format templates
        for template in raw_res:
            template['template_id'] = template['_id']
            await _format_template(template)

        return raw_res, total

    async def create_templates(self, templates_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        created_templates, failed_to_create : list
        """
        templates = []
        template_friendly_names = []
        for template in templates_batch:
            templates.append(
                tbl_templates(
                    template_id=template['template_id'],
                    account_id=template['account_id'],
                    friendly_name=template['friendly_name'],
                    template_type=template['template_type'],
                    title=template['title'],
                    content=template['content'],
                    default_values=template['default_values'],
                    metadata=template['metadata']
                )
            )
            template_friendly_names.append(template['friendly_name'])

        #BULK WRITE OPERATION
        invalid=[]
        bulk_operation = tbl_templates._get_collection().initialize_unordered_bulk_op()
        for template in templates:
            bulk_operation.insert(template.to_mongo())
        try:
            bulk_operation.execute()
        except Exception as bwe:
            print(bwe)
            for err in bwe.details['writeErrors']:
                invalid.append(err['op'].to_dict()['friendly_name'])

        valid = list(set(template_friendly_names) - set(invalid))

        failed_to_create=[]
        for in_ in invalid:
            failed_to_create.append(
                {
                    'friendly_name': in_,
                    'error_message' : f'Failed to created new message template. Template(s) with provided friendly_name(s) already exist. : {in_}'
                }
            )
        created_templates=[]
        for v in valid:
            template=next((d for i,d in enumerate(templates_batch) if v == d['friendly_name']),None)
            if template:
                template.pop('account_id', None)
                created_templates.append(template)
        
        return created_templates, failed_to_create

    async def update_templates(self, templates_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        updated_profiles, failed_to_update
        """
        updated_templates = []
        failed_to_update=[]
        not_existing_templates = []
        template_ids = [] # provided profile ids array

        # determine valid profiles
        for template in templates_batch:
            if template['template_id'] in template_ids:
                raise OctyException(400,'An error occurred when validating request.', [{'error_message' : f'Identical template identifers supplied. Found duplicate template_id : {template["template_id"]}', 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
            template_ids.append(template['template_id'])

        templates = json.loads(tbl_templates.objects(template_id__in=template_ids).to_json())
        if not templates:
            for template in templates_batch:
                failed_to_update.append(
                    {
                        'template_id' : template['template_id'],
                        'error_message' : f'No template found with template_id : {template["template_id"]}'
                    }
                )
            return updated_templates, failed_to_update
     

        for ti in template_ids:
            exists=next((key for key in templates if key['_id'] == ti), None)
            if not exists:
                template=next((key for key in templates_batch if key['template_id'] == ti), None)
                not_existing_templates.append(template['template_id'])
                failed_to_update.append(
                    {
                        'template_id': ti,
                        'error_message' : f'No template exists with provided template_id : {ti}'
                    }
                )

        #BULK UPDATE OPERATION
        bulk_operation = tbl_templates._get_collection().initialize_unordered_bulk_op()
        for t in templates:
            templates_batch_obj = next(key for key in templates_batch if key['template_id'] == t['_id'])

            # build update dict
            set_dict = DictConditional(lambda x: x != None)
            set_dict['_id'] = templates_batch_obj['template_id']
            set_dict['friendly_name'] = templates_batch_obj['friendly_name'] if templates_batch_obj['friendly_name'] != None else t['friendly_name']
            set_dict['template_type'] = templates_batch_obj['template_type'] if templates_batch_obj['template_type'] != None else t['template_type']
            set_dict['title'] = templates_batch_obj['title'] if templates_batch_obj['title'] != None else t['title']
            set_dict['content'] = templates_batch_obj['content'] if templates_batch_obj['content'] != None else t['content']
            set_dict['default_values'] = templates_batch_obj['default_values'] if templates_batch_obj['default_values'] != None else t['default_values']
            set_dict['metadata'] = templates_batch_obj['metadata'] if templates_batch_obj['metadata'] != None else t['metadata']
            set_dict['updated_at'] = dt.now()

            bulk_operation.find({
                '$and' : [
                    {"_id" : { "$eq" : t['_id']}},
                    {"account_id" : { "$eq" : templates_batch_obj['account_id']}}
                ]
            }).update(
                {
                    "$set" : set_dict
                }
            )

            # append updated profile to return array
            templates_batch_obj['created_at'] = t['created_at']
            templates_batch_obj['updated_at'] = dt.now()
            ut = await _format_template(templates_batch_obj)
            updated_templates.append(ut)
        
        try:
            bulk_operation.execute()
        except Exception as bwe:
            for err in bwe.details['writeErrors']:
                if err['code'] == 11000:
                    mes = f"Another template exists with provided friendly_name : {err['op']['u']['$set']['friendly_name']}"
                else:
                    mes = f"Unknown error occurred when updating template with friendly_name : {err['op']['u']['$set']['friendly_name']}"
                failed_to_update.append({
                        'template_id' : err['op']['u']['$set']['_id'],
                        'friendly_name':  err['op']['u']['$set']['friendly_name'],
                        'error_message' : mes
                    })


                updated_templates = list(filter(lambda i : i['template_id'] != err['op']['u']['$set']['_id'], updated_templates))


        return updated_templates, failed_to_update

    async def delete_templates(self, templates_batch : list) -> Union[list, list]:
        """
        Parameters
        ----------
        templates_batch : list

        Returns
        ----------
        deleted_templates, failed_to_delete : list
        """
        deleted_templates=[]
        failed_to_delete=[]
        template_ids=[]

        for template in templates_batch:
            template_ids.append(template['template_id'])


        templates = json.loads(tbl_templates.objects(template_id__in=template_ids).to_json())
        if not templates:
            for template in templates_batch:
                failed_to_delete.append(
                    {
                        'template_id' : template['template_id'],
                        'error_message' : f'No template found with template_id : {template["template_id"]}'
                    }
                )
            return deleted_templates, failed_to_delete
        
        bulk_operation = tbl_templates._get_collection().initialize_unordered_bulk_op()
        for template in templates_batch:
            t_object=next((key for key in templates if key['_id'] == template['template_id'] and key['account_id'] == template['account_id']), None)
            if t_object:
                deleted_templates.append(
                    {
                        'template_id': t_object['_id']
                    }
                )
            else:
                failed_to_delete.append(
                    {
                        'template_id' : template['template_id'],
                        'error_message' : f'No template found with template_id : {template["template_id"]}'
                    }
                )

            bulk_operation.find({
                '$and' : [
                    {  "_id" : { "$eq" : template['template_id'] }  },
                    {  "account_id" : { "$eq" : template['account_id'] }  }
                ]
            }).remove()

        bulk_operation.execute()

        return deleted_templates, failed_to_delete

    # delete templates to do with account_id
    async def delete_account_templates(self, account_id : str) -> bool:
        """
        Parameters
        ----------
        account_id : str

        Returns
        ----------
        bool
        """
        tbl_templates.objects(account_id__exact=account_id).delete()
        return True
    
async def _format_template(template : dict):
    '''
    Format template objects
    '''
    
    template.pop('_id', None)
    template.pop('account_id', None)
    try:
        template['created_at'] = int_to_dt(template['created_at']['$date'], as_str=True) if template['created_at'] != None else None
        try:
            template['updated_at'] = int_to_dt(template['updated_at']['$date'], as_str=True) if template['updated_at'] != None else None
        except TypeError:
            template['updated_at'] = template['updated_at'].strftime('%a, %d %b %Y %H:%M:%S GMT')
    except KeyError:
        pass
    return template


templatesRepository = _TemplatesRepository()