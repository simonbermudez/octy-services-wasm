from fastapi import Query
from pydantic import BaseModel, validator, HttpUrl
from typing import List, Dict, Optional
from email_validator import validate_email, EmailNotValidError

from config import Config


### Set account configs Input Schema
class SetAccountConfigs(BaseModel):
    account_id : Optional[str] = None
    contact_email_address : str
    @validator('contact_email_address')
    def validate_email_address(cls, email, **kwargs):
        try:
            valid = validate_email(email)
            return valid.email
        except EmailNotValidError:
            raise ValueError('Invalid contact email address provided.')
    contact_name : str
    contact_surname : str
    webhook_url : HttpUrl
    authenticated_id_key : Optional[str] = None


### Set recommendations algorithm configs Input Schema
class RecConfigs(BaseModel, allow_mutation=True):
    recommend_interacted_items : bool
    item_id_stop_list : List[str]
    profile_features : List[str]
    @validator('profile_features')
    def allowed_profile_features(cls, value, **kwargs):
        not_allowed = ["charged"]
        for v in value:
            if v in not_allowed:
                raise ValueError('Unable to set configurations for recommendations algorithm. \'charged\' can not be set as a profile feature.')
        return value
    # User will not set these configurations
    event_type : Optional[str] = 'charged'
    rec_item_identifier : Optional[str] = 'item_id'
class SetRecAlgoConfigs(BaseModel):
    account_id : Optional[str] = None
    algorithm_name : str
    configurations : RecConfigs

### Set churn prediction algorithm configs Input Schema
class ChurnPredConfigs(BaseModel, allow_mutation=True):
    profile_features : List[str]
    @validator('profile_features')
    def allowed_profile_features(cls, value, **kwargs):
        not_allowed = ["charged"]
        for v in value:
            if v in not_allowed:
                raise ValueError('Unable to set configurations for recommendations algorithm. \'charged\' can not be set as a profile feature.')
        return value
    # User will not set these configurations
    event_type : Optional[str] = 'charged'
    churn_item_identifier : Optional[str] = 'item_id'
class SetChurnAlgoConfigs(BaseModel):
    account_id : Optional[str] = None
    algorithm_name : str
    configurations : ChurnPredConfigs

class BaseSetAlgoConfigs(BaseModel):
    algorithm_name : str
    @validator('algorithm_name')
    def allowed_algorithms(cls, value, **kwargs):
        if value not in Config['OCTY_ALGO_TYPES']:
            raise ValueError('Invalid algorithm name provided. Allowed algorithm names : \'rec\' or \'churn\'')
        return value
    configurations : Dict
    
