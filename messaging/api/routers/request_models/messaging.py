from fastapi import Query
from pydantic import BaseModel, validator
from typing import Optional, Dict, List, Any
from config import Config
import re


def _template_content_validation(v):
    for t in v:
        required_data = []
        placeholder_tags = re.finditer(r"\{\{(.*?)\}\}", t.content, re.MULTILINE | re.DOTALL)
        for _, match in enumerate(placeholder_tags):
            for _ in range(0, len(match.groups())):
                    required_data.append(match.group(1))

        if len(required_data) > Config['MAX_REQUIRED_DATA']:
            raise ValueError(f"Template : {t.friendly_name}. A maximum number of {Config['MAX_REQUIRED_DATA']} placeholder tags allowed per template.")        

        default_value_keys = [key for key, _ in t.default_values.items()]

        df_rd = list(set(default_value_keys) - set(required_data))
        rd_df = list(set(required_data) - set(default_value_keys))
        if df_rd != [] or rd_df != []:
            raise ValueError(f"Template : {t.friendly_name}. Please ensure the placeholder tags set in the 'content' parameter match the values provided in the 'default_values' parameter. Found mismatches : {df_rd if df_rd != [] else rd_df}")    
    return v

def _metadata_validation(val):
    for k, v in val.items():
        if not isinstance(k, str):
            raise ValueError('Metadata keys must be of type: string.')
        
        if len(k) > 40 or len(k) < 1:
            raise ValueError('Metadata keys must be at least 1 character long and less than 40 characters long.')
        
        if len(str(v)) > 500 or len(str(v)) < 1:
            raise ValueError('Metadata values must be at least 1 character long and less than 500 characters long.')
    return val

### Create messaging templates Input Schema
class CreateTemplatesChild(BaseModel):
    friendly_name : str
    @validator('friendly_name')
    def validate_friendly_name(cls, value, **kwargs):
        # length
        if len(value) > 60 or len(value) < 1:
            raise ValueError('Message template friendly names must be at least 1 character long and less than 60 characters long.')
        # allowed characters
        disallowed_characters = [',', '"', "'", "."]
        found_characters = [c for c in disallowed_characters if c in value]
        if len(found_characters) > 0:
            raise ValueError(f'Illegal character(s) found in provided message template friendly name : {found_characters}')
        return value
    template_type : str
    title : str
    content : str
    default_values : Optional[Dict[str, str]] = {}
    @validator("default_values", pre=True, always=True)
    def set_default_values(cls, default_values):
        return default_values or {}
    metadata : Optional[Dict[str, Any]]
    @validator('metadata')
    def metadata_validation(cls, v):
        return _metadata_validation(v)

class CreateTemplates(BaseModel):
    templates : List[CreateTemplatesChild]
    @validator('templates')
    def length(cls, v):
        if len(v) > Config['MAX_CREATE_TEMPLATES']:
            raise ValueError(f'You can only create up to {Config["MAX_CREATE_TEMPLATES"]} templates per request.')
        return v
    @validator('templates')
    def content_validation(cls, v):
        return _template_content_validation(v)

### Update messaging templates Input Schema
class UpdateTemplatesChild(BaseModel):
    template_id : str
    friendly_name : str
    @validator('friendly_name')
    def validate_friendly_name(cls, value, **kwargs):
        # length
        if len(value) > 60 or len(value) < 1:
            raise ValueError('Message template friendly names must be at least 1 character long and less than 60 characters long.')
        # allowed characters
        disallowed_characters = [',', '"', "'", "."]
        found_characters = [c for c in disallowed_characters if c in value]
        if len(found_characters) > 0:
            raise ValueError(f'Illegal character(s) found in provided message template friendly name : {found_characters}')
        return value
    template_type : str
    title : str
    content : str
    default_values : Optional[Dict[str, str]] = {}
    @validator("default_values", pre=True, always=True)
    def set_default_values(cls, default_values):
        return default_values or {}
    metadata : Optional[Dict[str, Any]]
    @validator('metadata')
    def metadata_validation(cls, v):
        return _metadata_validation(v)

class UpdateTemplates(BaseModel):
    templates : List[UpdateTemplatesChild]
    @validator('templates')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_TEMPLATES']:
            raise ValueError(f'You can only update up to {Config["MAX_UPDATE_DELETE_TEMPLATES"]} templates per request.')
        return v
    @validator('templates')
    def content_validation(cls, v):
        return _template_content_validation(v)

### Delete messaging templates Input Schema
class DeleteTemplates(BaseModel):
    template_ids : List[str]
    @validator('template_ids')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_TEMPLATES']:
            raise ValueError(f'You can only delete up to {Config["MAX_UPDATE_DELETE_TEMPLATES"]} templates per request.')
        return v

### Generate content Input Schema
class GenerateContentChild(BaseModel):
    template_id : str
    data : List[Dict[str, Optional[str]]] 

class GenerateContent(BaseModel):
    messages : List[GenerateContentChild]
    @validator('messages')
    def length(cls, v):
        if len(v) > Config['MESSAGE_GEN_LIMIT']:
            raise ValueError('You can only generate up to 100 messagess per request.')
        return v
    @validator('messages')
    def content_validation(cls, v):
        v = ValidateMessageContent(v).validate()
        v = ValidateItemRecMessageContent(v).validate()
        v = ValidateRybbonMessageContent(v).validate()
        return v


class ValidateMessageContent:
    def __init__(self, value):
        self.value = value
        self.templates = list()

    def validate(self):
        for midx, message in enumerate(self.value):
            self.templates.append(message.template_id)
            err = self._validate_data_count(message.data)
            if err != '':
                raise ValueError(f"loc: messages : {midx}{err}")

        err = self._validate_duplicate_templates()
        if err != '':
            raise ValueError(f"loc: messages : {err}")
        return self.value

    def _validate_data_count(self, message_data) -> str:
        if len(message_data) > Config['MAX_MESSAGE_DATA']:
            return f". A maximum number of {Config['MAX_MESSAGE_DATA']} data objects allowed per message."
        return ""

    def _validate_duplicate_templates(self) -> str:
        if len(self.templates) != len(set(self.templates)):
            return "Duplicate template identifiers found in messages."
        return ""

class ValidateItemRecMessageContent:

    def __init__(self, value):
        self.value = value
        self.message_profiles = []
        self.data_profiles = list()
    
    def validate(self):
        for midx, message in enumerate(self.value):
            is_rec = False
            for didx, d in enumerate(message.data):
                self.data_profiles *= 0
                for k in d.keys():
                    if "." in k: # Assume its an item_rec key as no others are allowed '.' character
                        is_rec = True
                        err = self._validate_item_rec_key_structure(k)
                        if err != '':
                            raise ValueError(f"loc: messages : {midx} -> data: {didx}{err}")
                        err = self._validate_contains_profile_id(k, d[k])
                        if err != '':
                            raise ValueError(f"loc: messages : {midx} -> data: {didx}{err}")
                        if "item_price" in k:
                            err = self._validate_item_price_params(d[k])
                            if err != '':
                                raise ValueError(f"loc: messages : {midx} -> data: {didx}{err}")
                if is_rec:
                    err = self._validate_data_matching_profiles()
                    if err != '':
                        raise ValueError(f"loc: messages : {midx} -> data: {didx}{err}")
            if is_rec:
                err = self._validate_duplicate_message_data_profiles(len(message.data))
                if err != '':
                    raise ValueError(f"loc: messages : {midx} {err}")
            self.message_profiles *= 0
        return self.value

    def _validate_item_rec_key_structure(self, key) -> str:
        if key.count('.') > 1 or key.count('.') < 1:
            return f" -> key: '{key}'. item_rec keys must contain only one '.' character seperating the keyword 'item_rec' and the specified item attribute."
        params = key.split('.')
        if params[0] != 'item_rec':
            return f" -> key: '{key}'. item_rec keys must contain only one '.' character seperating the keyword 'item_rec' and the specified item attribute."
        if params[1] not in Config['ITEM_ATTRIBUTES']:
            return f" -> key: '{key}'. Illegal item attribute provided: '{params[1]}'. Allowed item attributes : {Config['ITEM_ATTRIBUTES']}"
        return ""

    def _validate_contains_profile_id(self, key, value) -> str:
        if "item_price" in key:
            try:
                value = value.split("::")[0]
            except: 
                return f" -> key: '{key}'. item_rec.item_price key values must contain '::' seperated values using the following sytax: profile_id::currency_from::currency_to::discount"
        if not re.match(r'[p][r][o][f][i][l][e][_][a-zA-Z0-9]',value):
            return f" -> key: '{key}'. item_rec key values must contain a valid Octy generated profile identifier as their first value."

        self.data_profiles.append(value)
        if value not in self.message_profiles:
            self.message_profiles.append(value)
        return ""

    def _validate_data_matching_profiles(self) -> str:
        if not len(set(self.data_profiles)) <= 1:
            return ". All profile identifiers provided within any single data object must match"
        return ""

    def _validate_duplicate_message_data_profiles(self, data_count) -> str:

        if len(set(self.message_profiles)) != data_count:
            return ". Identical profile identifiers found across more than one data object. Each data object within any message object should represent one profile or person."
        return ""

    def _validate_item_price_params(self, value) -> str:
        params = value.split("::")
        if len(params) != 4:
            return f" . Invalid value provided for item_price parameter. item_price parameters must contain four values separated by three sets of '::'. Expected : profile_id::currency_from::curency_to::discount. Value provided : {value}"
        
        for i, param in enumerate(params):
            if i == 0:
                # profile id check
                if not re.match(r'[p][r][o][f][i][l][e][_][a-zA-Z0-9]', param):
                    return f". Invalid value provided for item_price 'profile_id' parameter. Must be a valid Octy profile identifier. Value provided : {param}"
            elif i == 1:
                try:
                    Config['ALLOWED_CURRENCIES'][param]
                except KeyError:
                    return f". Invalid value provided for the item_price 'currency_from' parameter. Must be a valid accpeted currency code : {Config['ALLOWED_CURRENCIES']}. Value provided : {param}"
            elif i == 2:
                try:
                    Config['ALLOWED_CURRENCIES'][param]
                except KeyError:
                    return f". Invalid value provided for the item_price 'currency_to' parameter. Must be a valid accpeted currency code : {Config['ALLOWED_CURRENCIES']}. Value provided : {param}"
            elif i == 3:
                try:
                    int(param)
                    if 0 <= int(param) <= 90:
                        pass
                    else:
                        int("hi") # deliberately raise value error if value < 0 or > 90
                except ValueError:
                    return f". Invalid value provided for item_price 'discount' parameter. Must be a number greater than or equal to (if no discount is to be applied) 0. Value provided : {param}"
        return ""

class ValidateRybbonMessageContent:

    def __init__(self, value):
        self.value = value
        self.message_customer_ids = list()
        self.is_reward_card = False

    def validate(self):
        for midx, message in enumerate(self.value):
            for didx, d in enumerate(message.data):
                for k in d.keys():
                    if k == "rybbon_reward_card":
                        self.is_reward_card = True
                        err = self._validate_rybbon_params(d[k])
                        if err != '':
                            raise ValueError(f"loc: messages : {midx} -> data: {didx}{err}")

            if self.is_reward_card:
                err = self._validate_duplicate_message_data_customers(len(message.data))
                if err != '':
                    raise ValueError(f"loc: messages : {midx} {err}")
            self.message_customer_ids *= 0

        return self.value

    def _validate_rybbon_params(self, value) -> str:
        params = value.split("::")
        if len(params) != 3:
            return f" . Invalid value provided for rybbon_reward_card parameter. 'rybbon_reward_card' parameters must contain four values seperated by three sets of '::'. Expected : customer_id::rybbon_campaign_key::value. Value provided : {value}"
        
        for i, param in enumerate(params):
            if i == 0:
                if param == '' or param == None:
                    return f". Invalid value provided for rybbon_reward_card 'customer_id' parameter. Must not be a null value. Value provided : {param}"
                else:
                    self.message_customer_ids.append(param)
            elif i == 1:
                if param == '' or param == None:
                    return f". Invalid value provided for rybbon_reward_card 'rybbon_campaign_key' parameter. Must not be a null value. Value provided : {param}"
            elif i == 2:
                if param == '' or param == None:
                    return f". Invalid value provided for rybbon_reward_card 'reward_name' parameter. Must not be a null value. Value provided : {param}"
            elif i == 3:
                try:
                    float(param)
                    if float(param) < 1:
                        int("hi") # deliberately raise value error if value < 1
                except ValueError:
                    return f". Invalid value provided for rybbon_reward_card 'value' parameter. Must be a floating point number greater than or equal to 1. Value provided : {param}"

        return ""

    def _validate_duplicate_message_data_customers(self, data_count) -> str:
        if len(set(self.message_customer_ids)) != data_count:
            return ". Identical customer identifiers found across more than one data object. Each data object within any message object should represent one customer or person."
        return ""
    
class DeleteAccountMessaging(BaseModel):
    account_id : str