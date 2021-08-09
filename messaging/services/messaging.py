# module imports
from data.repositories.implementation.templates_repository import templatesRepository
from data.repositories.implementation.messaging_repository import messagingContentRepository
from api.routers.request_models.messaging import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *
import json

# external imports

class MessagingService():
    """
        MessagingService
        Handles:
        - Get templates
        - template creation
        - Update templates
        - Delete templates
        - Generate messages
        ...

        Attributes
        ----------
        account : Octy account
    """
    def __init__(self, account : Account): 
        self.account = account

    async def get_templates(self,
                    id_ : str = None, 
                    cursor : int = None) -> Union[dict, int]: 
        """
        Parameters
        ----------
        id_ : str
            template_id or friendly_name
        cursor : int
            Pagination cursor

        Returns
        ----------
        templates : dict
        total : int
        """

        if id_ != None and cursor == 0:
            template = await templatesRepository.get_templates(account_id=self.account.account_id, _id=id_)
            if not template[0]:
                raise OctyException(400, 'Invalid template identifier provided', 
                [{'message' : 'No templates were found with the provided identifier', 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
            
            return template[0], 1
            

        elif id_ == None and cursor != None:
            
            templates, total = await templatesRepository.get_templates(account_id=self.account.account_id, cursor=cursor)
            if len(templates)<1:
                raise OctyException(400, 'No templates found', 
                [{'message' : 'No templates found with the provided query parameters or pagination cursor exhausted', 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
            return templates, total

    async def create_templates(self, templates : CreateTemplates) -> Union[list, list]:
        """
        Parameters
        ----------
        templates : CreateTemplates
            CreateTemplates request model instance

        Returns
        ----------
        Created and failed to create templates : list, list
        """

        # assess allowed limits
        res, counts = assess_resource_limit(self.account.account_configurations['li'],
                              templatesRepository.get_template_count(self.account.account_id),
                              len(templates.templates))
        if not res:
            raise OctyException(400,'Resource limit exceeded', 
            [{'message' : f'This request could not be completed as the number of templates sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} templates.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        templates_batch = []
        for template in templates.templates:
            templates_batch.append(
                {
                    'template_id' : generate_uid('template'),
                    'account_id' : self.account.account_id,
                    'friendly_name' : template.friendly_name,
                    'template_type' : template.template_type,
                    'title' : template.title,
                    'content' : template.content,
                    'required_data' : template.required_data,
                    'default_values' : template.default_values
                }
            )

        created, failed = await templatesRepository.create_templates(templates_batch)

        if len(created) < 1:
            raise OctyException(400, 'No templates created!', failed)

        return created, failed

    async def update_templates(self, templates : UpdateTemplates) -> Union[list, list]:
        """
        Parameters
        ----------
        templates : UpdateTemplates
            UpdateTemplates request model instance

        Returns
        ----------
        Updated and failed to update templates : list, list
        """
        templates_batch = []
        for template in templates.templates:
            templates_batch.append(
                {
                    'template_id' : template.template_id,
                    'account_id' : self.account.account_id,
                    'friendly_name' : template.friendly_name,
                    'template_type' : template.template_type,
                    'title' : template.title,
                    'content' : template.content,
                    'required_data' : template.required_data,
                    'default_values' : template.default_values
                }
            )

        updated, failed = await templatesRepository.update_templates(templates_batch)

        if len(updated) < 1:
            raise OctyException(400, 'No templates updated!', failed)

        return updated, failed

    async def delete_templates(self, templates : DeleteTemplates) -> Union[list, list]:
        """
        Parameters
        ----------
        profiles : DeleteTemplates
            DeleteTemplates request model instance
    
        Returns
        ----------
        Deleted and failed to delete templates : list, list
        """
        templates_batch=[]
        for ti in templates.template_ids:
            templates_batch.append({
                "template_id" : ti,
                "account_id" : self.account.account_id
            })

        deleted , failed = await templatesRepository.delete_templates(templates_batch)

        if len(deleted) < 1:
            raise OctyException(400, 'No templates deleted!', failed)
        return deleted, failed

    async def _filter_templates(self, template_id : str, templates : list) -> list:
        return list(filter(lambda x : x['template_id'] == template_id, templates))

    async def _filter_items(self, item_id : str, items : list) -> list:
        return list(filter(lambda x : x['item_id'] == item_id, items))

    async def generate_message_content(self, messages : GenerateContent) -> Union[list, list, list]:
        """
        Parameters
        ----------
        messages : GenerateContent
            GenerateContent request model instance
    
        Returns
        ----------
        created_messages, failed_messages, failed_templates : list, list, list
        """
        failed_templates = [] #un-found templates
        err_templates_ids=[]
        failed_messages=[] #failed messages -- due to invalid data
        created_messages = [] #successfully created messages
        profile_id_template_rec_map=[] #item rec map used to map item recommendations to 
        rec_profile_ids=[] #profile_ids used for item rec 
        get_items=False

        templates = await templatesRepository.get_all_templates(account_id=self.account.account_id)
        if len(templates) < 1:
            raise OctyException(400, 'Template resource not found', 
            [{'message' : 'No templates exist for this account', 
            'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
        
        #determine if template exists
        for message in messages.messages:
            t = await self._filter_templates(message.template_id,templates )
            if len(t) == 0:
                err_templates_ids.append(message.template_id)
                failed_templates.append({'template_id' : message.template_id, 'reason' : 'No template found with this template_id. All messages using this template_id were not created.'})
            else:
                if not message.item_recommendation:
                    continue
                for d in message.data:
                    for k,v in d.items():
                        if k=='profile_id':
                            profile_id_template_rec_map.append(
                                {
                                    'profile_id' : v,
                                    'template_id' : message.template_id,
                                    'rec_item_id' : None
                                }
                            )
                            rec_profile_ids.append(v)

        if len(rec_profile_ids)>=1: #batch get recommendations
            item_recommendations = await messagingContentRepository\
                .get_item_recommendations(account_id=self.account.account_id, profile_ids=rec_profile_ids)
            
            if len(item_recommendations) < 1:
                #all templates with 'item_recommendation'==true, append to failed templates.
                for i in profile_id_template_rec_map:
                    if i['template_id'] not in err_templates_ids:
                        failed_templates.append({
                            'template_id' : i['template_id'],
                            'reason' : 'Item recommendations failed to be processed for this template. All messages using this template_id were not created. This is possibly due to no valid profile_id(s) being provided or no trained recommendations models are currently available on this account.'
                        })
                        err_templates_ids.append(i['template_id'])
            else:
                #map recommendations to profile_id_template_rec_map
                for i, rec in enumerate(item_recommendations):
                    if rec['error'] != None:
                        continue
                    #obj=next(key for key in profile_id_template_rec_map if key['profile_id'] == rec['profile_id'])
                    obj_index=next((index for (index, d) in enumerate(profile_id_template_rec_map) if d["profile_id"] == rec['profile_id']), None)
                    if obj_index==None:
                        continue
                    profile_id_template_rec_map[obj_index]['rec_item_id']=rec['recommendations'][0]['item_id']
                get_items=True
        
        if get_items:
            #we need to access all items associated with this account.
            items = await messagingContentRepository\
                .get_items(account_id=self.account.account_id)

        #iterate over existing templates --
        for template in templates:

            #iterate over requested messages
            for message in messages.messages:

                if message.template_id in err_templates_ids:
                    continue

                if template['template_id'] == message.template_id:

                    #if the current template 'item_recommendation' parameter is True, the current template object must contain a 'ITEM_REC' placeholer
                    if message.item_recommendation == True:
                        
                        if 'ITEM_REC' not in  template['content']:
                            failed_templates.append({'template_id' : message.template_id, 'reason' : 'No \'ITEM_REC\' placeholder set in this template. Either set \'item_recommendation\' to \'false\' or add required placeholder to template.'})
                            err_templates_ids.append(message.template_id)
                            continue
                    else:
                        if 'ITEM_REC' in  template['content']:
                            failed_templates.append({'template_id' : message.template_id, 'reason' : '\'ITEM_REC\' placeholder set in this template. Either set \'item_recommendation\' to \'true\' or remove ITEM_REC placeholder from template.'})
                            err_templates_ids.append(message.template_id)
                            continue


                    #iterate over each message data object -- (each requested message)
                    for d in message.data:
                        values_dict = {} #init values dict
                        message_failed=False
                        #if if the current template 'item_recommendation' parameter is True, each message data object must contain a 'profile_id' key
                        if message.item_recommendation == True:
                            try:
                                d['profile_id']
                            except KeyError:
                                #dummy values are not provided with product recommendations placeholders. 
                                failed_messages.append({'provided_data' : d, 'reason' : '\'item_recommendation\' parameter set to \'true\' on this message set. Missing required data parameter for this message. \'profile_id\''})
                                continue
                        
                        #if item_recommendation, get recommendations and append to values dict.
                        if message.item_recommendation == True:
                            if get_items: #only if we have items to filter
                                try:
                                    values_dict['ITEM_REC']
                                except KeyError:
                                    #get item_id from profile_id_template_rec_map
                                    item_id=next(key for key in profile_id_template_rec_map if key['profile_id'] == d['profile_id'])['rec_item_id']
                                    if not item_id:
                                        failed_messages.append({'profile_id' : d['profile_id'], 'reason' : 'Failed to get recommended item for this profile'})
                                        continue
                                    #filter item object
                                    rec_items = await self._filter_items(item_id, items)
                                    if len(rec_items)<1:
                                        failed_messages.append({'profile_id' : d['profile_id'], 'reason' : 'Failed to get recommended item for this profile'})
                                        continue
                                    values_dict['ITEM_REC'] = rec_items[0]['item_name']
                        
                        #iterate over required data keys in template
                        for key in template['required_data']:

                            #verify current message data object contains current required data key
                            try:
                                d[key]
                            except KeyError:
                                failed_messages.append({'provided_data' : d, 'reason' : 'Missing required data parameter for this message. \'{}\''.format(key)})
                                message_failed=True
                                break
                            
                            if d[key] == "":
                                values_dict[key] = str(template['default_values'][key])
                            else:
                                values_dict[key] = str(d[key])
                            

                        if message_failed:
                            #got to next data object
                            continue

                        content = template['content'].format(**values_dict)
                        created_messages.append({'template_id' : message.template_id, 'friendly_name' : template['friendly_name'],'title' : template['title'] ,'content' : content})

        return created_messages, failed_messages, failed_templates
