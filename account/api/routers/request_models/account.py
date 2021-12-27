from fastapi import Query
from pydantic import BaseModel, validator, HttpUrl
from typing import List, Dict
from email_validator import validate_email, EmailNotValidError


### Create account Input Schema
class CreateAccount(BaseModel):
    contact_email_address : str
    @validator('contact_email_address')
    def validate_email_address(cls, email, **kwargs):
        try:
            valid = validate_email(email)
            return valid.email
        except EmailNotValidError:
            raise ValueError('Invalid contact email address provided.')
    account_name : str
    account_type : str
    @validator('account_type')
    def allowed_account_types(cls, value, **kwargs):
        allowed = ["startup", "pro", "enterprise"]
        if value not in allowed:
            raise ValueError('Invalid account type provided. Allowed permissions : \'startup\', \'pro\' or \'enterprise\'')
        return value
    account_currency : str
    contact_name : str
    contact_surname : str
    webhook_url : HttpUrl
    permissions : List[str]
    @validator('permissions')
    def allowed_permissions(cls, value, **kwargs):
        allowed = ["rec", "churn", "rfm", "seg", "mes"]
        for v in value:
            if v not in allowed:
                raise ValueError('Invalid permission provided. Allowed permissions : \'rec\', \'churn\' or \'rfm\' or \'seg\' or \'mes\'')
        return value

### Get accounts internal Input Schema
class GetAccountsInternal(BaseModel):
    account_ids : list