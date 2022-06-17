# module imports
from re import template
from this import s
from data.repositories.implementation.templates_repository import templatesRepository
from data.repositories.implementation.messaging_repository import messagingContentRepository
from api.routers.request_models.messaging import *
from api.routers.request_models.account import Account
from api.routers.error_handlers import *
from utils.utils import *
from config import Config

# python imports
from typing import *

# external imports
from stuf import stuf
from currencies import Currency


class TemplateEngine():
    """
        TemplateEngine
        Handles:
        - Generating message content from specified templates and user supplied data
        ...

        Attributes
        ----------
        account : Octy account
    """
    def __init__(self, account : Account): 
        self.account = account
        self.all_templates = None
        self.working_templates = list()
        self.failed_messages = list()
        self.failed_templates = list()
        self.failed_template_ids = list()
        self.templates_required_data = list()
        self.currency_rates = None
        self.item_recommendations = list()
        self.items = list()
        self.created_messages = list()
        self.profile_item_map = list()

    def _handle_template_err(self, template_id : str, err_msg : str) -> None:
        self.failed_template_ids.append(template_id)
        self.failed_templates.append({'template_id' : template_id, 'error_message' : err_msg})

    def _handle_message_err(self, template_id : str, provided_data : object, err_msg : str) -> None:
        self.failed_messages.append({'template_id': template_id, 'provided_data' : provided_data, 'error_message' : err_msg})

    def _handle_created_message(self, template_id : str, friendly_name : str, title : str, content : str) -> None:
        self.created_messages.append(
            {   'template_id' : template_id, 
                'friendly_name' : friendly_name,
                'title' : title ,
                'content' : content
            }
        )

    async def _get_all_templates(self) -> list:
        self.all_templates = await templatesRepository.get_all_templates(account_id=self.account.account_id)
        if len(self.all_templates) < 1:
            raise OctyException(400, 'Template resource not found', 
            [{'error_message' : 'No templates exist for this account', 
            'extended_help': Config['MESSAGING_EXTENDED_HELP']}])

    def _verify_template_exist(self, template_id : str) -> None:
        def _filter_templates(template_id : str, templates : list) -> list:
            return list(filter(lambda x : x['template_id'] == template_id, templates))
        filtered = _filter_templates(template_id, self.all_templates)
        if len(filtered) < 1:
            self._handle_template_err(template_id, 'No template found with this template_id. All messages using this template_id were not created.')
        self.working_templates.append(filtered[0])

    def _identify_required_data(self, template_id : str, content : str) -> list:
        required_data = []
        placeholder_tags = re.finditer(r"\{\{(.*?)\}\}", content, re.MULTILINE | re.DOTALL)
        for _, match in enumerate(placeholder_tags):
            for _ in range(0, len(match.groups())):
                required_data.append(match.group(1))
        return {
            "template_id": template_id,
            "required_data": required_data
        }

    async def _parse_group_profile_ids(self, messages : Any) -> list:
        profile_ids = list()
        def parse(obj, key):
            try:
                if "item_rec" in key:
                    if "item_price" in key: 
                        params = obj[key].split("::")
                        return params[0]
                    return obj[key]
            except KeyError:
                pass
        for tr in self.templates_required_data:
            for r in tr["required_data"]:
                for m in messages:
                    for d in m.data:
                        p = parse(d, r)
                        if p and p not in profile_ids: profile_ids.append(p)
        return list(dict.fromkeys(profile_ids))

    async def _get_currency_rates(self) -> None:
        if next((r for r in self.templates_required_data if any("item_price" in rd for rd in r['required_data'])),None):
            self.currency_rates = await messagingContentRepository.get_currency_rates()

    async def _get_recommendedations(self, profile_ids : str) -> None:
        self.item_recommendations = await messagingContentRepository\
            .get_item_recommendations(account_id=self.account.account_id, profile_ids=profile_ids)

    async def _filter_items(self, item_id : str, items : list) -> list:
        return list(filter(lambda x : x['item_id'] == item_id, items))

    async def _get_items(self) -> None:
        self.items = await messagingContentRepository\
            .get_items(account_id=self.account.account_id)

    async def build_profile_item_map(self) -> None:
        for rec in self.item_recommendations:
            if len(rec['recommendations'])<1: continue
            profile_item = {}
            profile_item['profile_id'] = rec['profile_id']
            top_item = rec['recommendations'][0]['item_id']
            item = next((i for i in self.items if i['item_id'] == top_item), None)
            if item:
                profile_item['item'] = item
                self.profile_item_map.append(profile_item)

    async def _format_template_placeholder_tags(self, template : dict) -> dict:
        template['content'] = template['content'].replace("{{", "{")
        template['content'] = template['content'].replace("}}", "}")
        return template

    async def _generate(self, message : object, template : object): 

        template_required_data=next((key for key in self.templates_required_data if key['template_id'] == message.template_id), None)

        for data in message.data:
            values_dict = {} # content dynamic values
            values_dict['item_rec'] = {} # item rec values
            message_failed=False

            # init item rec here
            item_rec = ItemRec(
                data, 
                template,
                template_required_data['required_data'],
                self.profile_item_map,
                self.currency_rates)

            for key in template_required_data['required_data']:
                try:
                    value = data[key]
                except KeyError:
                    self._handle_message_err(
                        message.template_id,
                        data,
                        f'Missing required data parameter for this message: \'{key}\''
                    )
                    message_failed=True
                    break
                if "item_rec" in key:
                    values_dict = item_rec.populate_values_dict(values_dict, key, value)
                    continue
                if value == "" or value == None:
                    values_dict[key] = str(template['default_values'][key])
                else:
                    values_dict[key] = str(value)

            if message_failed:
                continue
            template = await self._format_template_placeholder_tags(template)
            if item_rec.has_rec:
                content = template['content'].format(**stuf(values_dict))
            else:
                content = template['content'].format(**values_dict)
            self._handle_created_message(message.template_id, template['friendly_name'], template['title'], content)
    
    async def generate(self, messages : GenerateContent) -> None: 
        """
        Parameters
        ----------
        messages : GenerateContent
            GenerateContent request model instance
    
        Returns
        ----------
        None (Access object instance properties for results if no expection thrown)
        """

        # Determine if any templates exist and return them
        await self._get_all_templates()
        # Verify provided templates exist
        for message in messages.messages: self._verify_template_exist(message.template_id)
        
        # Get working templates required data
        for t in self.working_templates :
            self.templates_required_data.append(
                self._identify_required_data(t.id, t.content)
            )

        # Determine if templates have any item_rec placeholder tags
        if next((r for r in self.templates_required_data if any("item_rec" in rd for rd in r['required_data'])),None):
            # Get Item recommendations for all templates profiles
            profile_ids = await self._parse_group_profile_ids(messages.messages)
            await self._get_recommendedations(profile_ids)
            # get all items
            await self._get_items()
            if len(self.items) > 0:
                # Get currecny rates
                await self._get_currency_rates()
            # buil profile item map for populating item attributes into content
            await self.build_profile_item_map()
        
        for template in self.working_templates:
            for message in messages.messages: 
                if message.template_id in self.failed_template_ids: continue
                if template['template_id'] == message.template_id:
                    await self._generate(message, template)


class ItemRec:
    '''
        Handle adding recommended item values to the values dict
    '''
    def __init__(self, data, template, required_data, profile_item_map, currency_rates):
        self.data = data
        self.template = template
        self.required_data = required_data
        self.profile_item_map = profile_item_map
        self.currency_rates = currency_rates
        self._has_rec = self._has_item_reccomendations()
        self.item = None
        if self._has_rec:
            self._get_rec_item()
    
    @property
    def has_rec(self):
        return self._has_rec

    def _has_item_reccomendations(self) -> bool:
        if any("item_rec" in rd for rd in self.required_data):
            return True
        return False
    
    def _get_rec_item(self):
        first_item_param = next(t for t in self.required_data if "item_rec" in t)
        profile_id = self.data[first_item_param]
        self.item = next((t for t in self.profile_item_map if t['profile_id'] == profile_id), None)
    
    def _parse_item_rec_param(self, param : str, item : dict):
        '''
            Parse string representation of an
            item attribute and return relative value
            ex: 'item.item_price' -> 300
        '''
        return item[param.split(".")[1]]

    def populate_values_dict(self, values_dict, key, value) -> dict:
        '''
            Populate item rec values in values_dict and return
        '''
        if self.item:
            item_value = self._parse_item_rec_param(key, self.item['item'])
            if "item_price" in key:
                item_value = ItemPrice(value, item_value, self.currency_rates).format()
            values_dict['item_rec'][key.split(".")[1]] = str(item_value)
        else:
            values_dict['item_rec'][key.split(".")[1]] = str(self.template['default_values'][key])
        
        return values_dict

class ItemPrice:
    '''
        Handle currency conversion, formatting and dynamic item price discounts
    '''
    def __init__(self, params : str, amount : int, currency_rates : dict):
        self.params  = params
        self.amount = amount / 100
        self.currency_rates = currency_rates


    def _currency_conversion(self, currency_from, currency_to, amount):
        if currency_from == currency_to:
            return round(amount, 2)
        base = next((self.currency_rates[k] for k,_ in self.currency_rates.items() if k == currency_to), None)
        exchange = next((base['rates'][k] for k,_ in base['rates'].items() if k == currency_from), None)
        return round(amount * exchange, 2)


    def format(self) -> str:
        self.params = self.params.split("::")
        # determine if discount is required
        if int(self.params[3]) > 0:
            discount = (int(self.params[3]) / 100) * self.amount
            self.amount -= discount

        if self.params[1] == self.params[2]: # no currency conversion required
            self.amount = Currency(self.params[1].upper()).get_money_format(int(self.amount)/100)
        else:
            self.amount = Currency(self.params[2].upper()).get_money_format(self._currency_conversion(self.params[1], self.params[2], int(self.amount)/100))

        return self.amount