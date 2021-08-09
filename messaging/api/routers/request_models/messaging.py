from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config
import re

def _content_validation(v):
    for t in v:
        required_data = []
        default_values = []
        content_placeholders = []

        for r in t.required_data:
            if r != 'ITEM_REC': #default values do not need to be set for ITEM_REC placeholders.
                required_data.append(r)

        for key, _ in t.default_values.items():
            default_values.append(key)


        placeholders = re.finditer(r"\{(.*?)\}", t.content, re.MULTILINE | re.DOTALL)

        for _, match in enumerate(placeholders):
            for _ in range(0, len(match.groups())):
                content_placeholders.append(match.group(1))

        if 'ITEM_REC' in content_placeholders:
            content_placeholders.remove('ITEM_REC')

        # Ensure content contains each of the required data point placeholders
        y = list(set(required_data) - set(content_placeholders))
        if y != []:
            raise ValueError(f'Template : {t.friendly_name}. Please ensure the placeholders set in the content parameter match the values provided in the required data field. Found mismatches : {y}')

        z = list(set(content_placeholders) - set(required_data))
        if z != []:
            raise ValueError(f'Template : {t.friendly_name}. Please ensure the placeholders set in the content parameter match the values provided in the required data field. Found mismatches : {z}')

        # Check there is a default value for each required data point
        x = list(set(required_data) - set(default_values))
        if x != []:
            raise ValueError(f'Template : {t.friendly_name}. Please provide default values for the following required data placeholders : {x}')
    return v

### Create messaging templates Input Schema
class CreateTemplatesChild(BaseModel):
    friendly_name : str
    template_type : str
    title : str
    content : str
    required_data : List[str]
    default_values : Dict[str, str]

class CreateTemplates(BaseModel):
    templates : List[CreateTemplatesChild]
    @validator('templates')
    def length(cls, v):
        if len(v) > Config['MAX_CREATE_TEMPLATES']:
            raise ValueError('You can only create up to 100 templates per request.')
        return v
    @validator('templates')
    def content_validation(cls, v):
        return _content_validation(v)

### Update messaging templates Input Schema
class UpdateTemplatesChild(BaseModel):
    template_id : str
    friendly_name : str
    template_type : str
    title : str
    content : str
    required_data : List[str]
    default_values : Dict[str, str]

class UpdateTemplates(BaseModel):
    templates : List[UpdateTemplatesChild]
    @validator('templates')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_TEMPLATES']:
            raise ValueError('You can only update up to 100 templates per request.')
        return v
    @validator('templates')
    def content_validation(cls, v):
        return _content_validation(v)


class DeleteTemplates(BaseModel):
    template_ids : List[str]
    @validator('template_ids')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_TEMPLATES']:
            raise ValueError('You can only delete up to 100 templates per request.')
        return v

### Generate content Input Schema
class GenerateContentChild(BaseModel):
    template_id : str
    item_recommendation : bool
    data : List[Dict[str, str]]

class GenerateContent(BaseModel):
    messages : List[GenerateContentChild]
    @validator('messages')
    def length(cls, v):
        if len(v) > Config['MESSAGE_GEN_LIMIT']:
            raise ValueError('You can only generate up to 100 messagess per request.')
        return v