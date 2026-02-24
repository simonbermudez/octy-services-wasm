from fastapi import Query
from pydantic import BaseModel, ValidationError, validator
from typing import Optional, Dict, List, Any
from config import Config

def disallow_null_values(value : dict, attribute : str):
    for k, v in value.items():
        ex = f'Invalid {attribute} attribute provided. Null values or empty strings can not be provided as {attribute} values. Invalid key pair value: ({k} : {v})'
        if v is None :
            raise ValueError(ex)
        elif type(v) == str:
            if v == "" or v.isspace():
                raise ValueError(ex)
    return value

### Create customer profiles Input Schema
class CreateProfilesChild(BaseModel):
    customer_id : str
    @validator('customer_id')
    def validate_customer_id(cls, value, **kwargs):
        # length
        if len(value) > 60 or len(value) < 1:
            raise ValueError('Customer identifiers must be at least 1 character long and less than 60 characters long.')
        # allowed characters
        disallowed_characters = [',', '"', "'", "."]
        found_characters = [c for c in disallowed_characters if c in value]
        if len(found_characters) > 0:
            raise ValueError(f'Illegal character(s) found in provided customer identifier : {found_characters}')
        return value
    profile_data : Dict
    @validator('profile_data')
    def profiledata(cls, v):
        return disallow_null_values(v, 'profile_data')
    platform_info : Dict
    @validator('platform_info')
    def platforminfo(cls, v):
        return disallow_null_values(v, 'platform_info')
    has_charged : bool

class CreateProfiles(BaseModel):
    profiles : List[CreateProfilesChild]
    @validator('profiles')
    def length(cls, v):
        if len(v) > Config["MAX_CREATE_PROFILES"]:
            raise ValueError(f'You can only create up to {Config["MAX_CREATE_PROFILES"]} profiles per request. For larger uploads, please use the octy cli.')
        return v

### Update customer profiles Input Schema
class SegmentTags(BaseModel):
    segment_id : str
    segment_tag : str

class UpdateProfilesChild(BaseModel):
    profile_id : str
    customer_id : str
    @validator('customer_id')
    def validate_customer_id(cls, value, **kwargs):
        # length
        if len(value) > 60 or len(value) < 1:
            raise ValueError('Customer identifiers must be at least 1 character long and less than 60 characters long.')
        # allowed characters
        disallowed_characters = [',', '"', "'", "."]
        found_characters = [c for c in disallowed_characters if c in value]
        if len(found_characters) > 0:
            raise ValueError(f'Illegal character(s) found in provided customer identifier : {found_characters}')
        return value
    profile_data : Dict
    @validator('profile_data')
    def profiledata(cls, v):
        return disallow_null_values(v, 'profile_data')
    platform_info : Dict
    @validator('platform_info')
    def platforminfo(cls, v):
        return disallow_null_values(v, 'platform_info')
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
            raise ValueError(f'You can only update up to {Config["MAX_UPDATE_DELETE_PROFILES"]} profiles per request.')
        return v


### Delete customer profiles Input Schema
class DeleteProfiles(BaseModel):
    profiles : List[str]
    @validator('profiles')
    def length(cls, v):
        if len(v) > Config["MAX_UPDATE_DELETE_PROFILES"]:
            raise ValueError(f'You can only delete up to {Config["MAX_UPDATE_DELETE_PROFILES"]} profiles per request.')
        return v

### Get customer profiles Internal Input Schema
class GetProfilesInternal(BaseModel):
    account_id : str
    profiles : List[str]
    tag_statuses : Optional[List[str]] = ['active'] # the allowed status of segment tags returned with each profile
    get_all : bool # if true, service will ignore any sent profile ids.


class DeleteAccountProfiles(BaseModel):
    account_id : str