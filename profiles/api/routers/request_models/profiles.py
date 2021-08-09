from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config

'''
#description: Optional[str] = None
customer_id : str = Query(..., max_length=20) #... <- makes it required
'''

### Create customer profiles Input Schema
class CreateProfilesChild(BaseModel):
    customer_id : str
    profile_data : Dict
    platform_info : Dict
    has_charged : bool

class CreateProfiles(BaseModel):
    profiles : List[CreateProfilesChild]
    @validator('profiles')
    def length(cls, v):
        if len(v) > Config["MAX_CREATE_PROFILES"]:
            raise ValueError('You can only create up to 100 profiles per request. For larger uploads, please use the octy cli.')
        return v

### Update customer profiles Input Schema
class SegmentTags(BaseModel):
    segment_id : str
    segment_tag : str

class UpdateProfilesChild(BaseModel):
    profile_id : str
    customer_id : str
    profile_data : Dict
    platform_info : Dict
    has_charged : bool
    status: str
    @validator('status')
    def allowed_status(cls, value, **kwargs):
        choices = ['active', 'inactive', 'churned']
        if value not in choices:
            raise ValueError('Invalid status provided. Allowed statuses : \'active\', \'inactive\' or \'churned\'')
        return value
    rfm_score : Optional[int]
    rfm_segment_desc : Optional[str]
    churn_probability : Optional[str]
    ltv_prediction : Optional[int]
    current_ltv : Optional[int]
    segment_tags : Optional[List[SegmentTags]]

class UpdateProfiles(BaseModel):
    profiles : List[UpdateProfilesChild]
    @validator('profiles')
    def length(cls, v):
        if len(v) > Config["MAX_UPDATE_DELETE_PROFILES"]:
            raise ValueError('You can only update up to 100 profiles per request.')
        return v


### Delete customer profiles Input Schema
class DeleteProfiles(BaseModel):
    profiles : List[str]
    @validator('profiles')
    def length(cls, v):
        if len(v) > Config["MAX_UPDATE_DELETE_PROFILES"]:
            raise ValueError('You can only delete up to 100 profiles per request.')
        return v

### Get customer profiles Internal Input Schema
class GetProfilesInternal(BaseModel):
    account_id : str
    profiles : List[str]
    tag_statuses : Optional[List[str]] = ['active'] # the allowed status of segment tags returned with each profile
    get_all : bool # if true, service will ignore any sent profile ids.