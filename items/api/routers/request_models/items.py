from pydantic import BaseModel, validator
from typing import List
from config import Config

### Create items Input Schema
class CreateItem(BaseModel):
    item_id : str
    item_category : str
    item_name : str
    item_description : str
    @validator('item_description')
    def desc_allowed_len(cls, value, **kwargs):
        if len(value) > 40 or len(value) < 1:
            raise ValueError('Item description must be at least 1 character long and less than 40 characters long.')
        return value
    item_price : int

class CreateItems(BaseModel):
    items : List[CreateItem]
    @validator('items')
    def length(cls, v):
        if len(v) > Config['MAX_CREATE_ITEMS']:
            raise ValueError(f'You can only create up to {Config["MAX_CREATE_ITEMS"]} items per request. For larger uploads, please use the octy cli.')
        return v
    

### Update items Input Schema
class UpdateItem(BaseModel):
    item_id : str
    item_category : str
    item_name : str
    item_description : str
    @validator('item_description')
    def desc_allowed_len(cls, value, **kwargs):
        if len(value) > 40 or len(value) < 1:
            raise ValueError('Item description must be at least 1 character long and less than 40 characters long.')
        return value
    item_price : int
    status : str
    @validator('status')
    def allowed_statuses(cls, value, **kwargs):
        if value not in ['active', 'inactive']:
            raise ValueError('Invalid item status provided. Allowed statuses : \'active\', \'inactive\'')
        return value

class UpdateItems(BaseModel):
    items : List[UpdateItem]
    @validator('items')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_ITEMS']:
            raise ValueError(f'You can only update up to {Config["MAX_UPDATE_DELETE_ITEMS"]} items per request.')
        return v

### Delete items Input Schema
class DeleteItems(BaseModel):
    items : List[str]
    @validator('items')
    def length(cls, v):
        if len(v) > Config['MAX_UPDATE_DELETE_ITEMS']:
            raise ValueError(f'You can only delete up to {Config["MAX_UPDATE_DELETE_ITEMS"]} items per request.')
        return v