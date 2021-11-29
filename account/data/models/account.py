from pydantic import BaseModel, validator, HttpUrl
from email_validator import validate_email, EmailNotValidError
from typing import *

### Update account Input Schema
class algorithmConfig(BaseModel):
    algorithm_name : str
    config_json : Any


class churnInfo(BaseModel):
    churn_percentage : float
    churn_indicator : str
    churn_difference : float
    features : Optional[List[Any]] = None

class UpdateAccount(BaseModel):
    account_id : str
    contact_email_address : Optional[str] = None
    @validator('contact_email_address')
    def validate_email_address(cls, email, **kwargs):
        try:
            valid = validate_email(email)
            return valid.email
        except EmailNotValidError:
            raise ValueError('Invalid contact email address provided.')
    contact_name : Optional[str] = None
    contact_surname : Optional[str] = None
    webhook_url : Optional[HttpUrl] = None
    authenticated_id_key : Optional[str] = None
    algorithm_configurations : Optional[algorithmConfig] = None
    churn_info : Optional[churnInfo] = None