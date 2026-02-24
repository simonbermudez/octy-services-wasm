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
                    identifiers : list = None, 
                    cursor : int = None) -> Union[list, int]: 
        """
        Parameters
        ----------
        identifiers : list
            list of template_id(s) or friendly_name(s)
        cursor : int
            Pagination cursor

        Returns
        ----------
        templates : dict
        total : int
        """

        if identifiers != None and cursor == 0:
            templates, total = await templatesRepository.get_templates(account_id=self.account.account_id, identifiers=identifiers)
            if total<1:
                raise OctyException(400, 'Invalid template identifier(s) provided', 
                [{'error_message' : 'No templates were found with the provided identifier(s)', 
                'extended_help': Config['MESSAGING_EXTENDED_HELP']}])
            
            return templates, total
            

        elif identifiers == None and cursor != None:
            
            templates, total = await templatesRepository.get_templates(account_id=self.account.account_id, cursor=cursor)
            if len(templates)<1:
                raise OctyException(400, 'No templates found', 
                [{'error_message' : 'No templates found with the provided query parameters or pagination cursor exhausted', 
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
            [{'error_message' : f'This request could not be completed as the number of templates sent with this request exceeds the allowed limit of : {counts["limit"]}. This account can create another {counts["remainder"]} templates.', 'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

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
                    'default_values' : template.default_values,
                    'metadata' : template.metadata
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
                    'default_values' : template.default_values,
                    'metadata' : template.metadata
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
    
    # Delete all templates, messages and message content for an account
    async def delete_account_messaging_internal(self, account_id: str) -> bool:
        """
            A method used to delete all messaging data for an Octy account.

            Parameters
            ----------
            account_id : str
                Account unique identifier

            Returns
            ----------
            True if account was deleted successfully, False otherwise : bool
        """
        # Delete messages
        res = await messagingContentRepository.delete_messages(account_id)
        if res is False:
            raise Exception(500, 'Messages could not be deleted.')
        
        # Delete templates
        res = await templatesRepository.delete_templates_by_account_id(account_id)
        if res is False:
            raise Exception(500, 'Templates could not be deleted.')
        
        return True