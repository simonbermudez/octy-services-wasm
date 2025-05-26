from fastapi import Query
from pydantic import BaseModel, validator, HttpUrl
from typing import List, Dict, Union, Optional
from email_validator import validate_email, EmailNotValidError
from enum import Enum

class PlatformType(str, Enum):
    SHOPIFY = "shopify"
    WOOCOMMERCE = "woocommerce"
    BIGCOMMERCE = "bigcommerce"
    MAGENTO = "magento"
    PRESTASHOP = "prestashop"
    SQUARESPACE = "squarespace"
    CUSTOM = "custom"

class ConnectedPlatform(BaseModel):
    platform_type: PlatformType
    store_url: str
    store_name: str
    
    @validator('store_url')
    def validate_store_url(cls, v):
        if not v or len(v.strip()) == 0:
            raise ValueError('Store URL cannot be empty')
        return v.strip()
    
    @validator('store_name')
    def validate_store_name(cls, v):
        if not v or len(v.strip()) == 0:
            raise ValueError('Store name cannot be empty')
        return v.strip()

class CreateAccount(BaseModel):
    contact_email_address: str
    
    @validator('contact_email_address')
    def validate_email_address(cls, email, **kwargs):
        try:
            valid = validate_email(email)
            return valid.email
        except EmailNotValidError:
            raise ValueError('Invalid contact email address provided.')
    
    account_name: str
    account_type: str
    authenticated_id_key: Optional[str] = None
    
    @validator('account_type')
    def allowed_account_types(cls, value, **kwargs):
        allowed = ["startup", "pro", "enterprise"]
        if value not in allowed:
            raise ValueError('Invalid account type provided. Allowed permissions : \'startup\', \'pro\' or \'enterprise\'')
        return value
    
    account_currency: str
    contact_name: str
    contact_surname: str
    webhook_url: HttpUrl
    permissions: List[str]
    
    @validator('permissions')
    def allowed_permissions(cls, value, **kwargs):
        allowed = ["rec", "churn", "rfm", "seg", "mes"]
        for v in value:
            if v not in allowed:
                raise ValueError('Invalid permission provided. Allowed permissions : \'rec\', \'churn\' or \'rfm\' or \'seg\' or \'mes\'')
        return value
    
    # New field for platforms
    connected_platforms: Optional[List[ConnectedPlatform]] = []
    
    @validator('connected_platforms')
    def validate_platforms(cls, platforms):
        if not platforms:
            return platforms
        
        store_urls = [str(p.store_url) for p in platforms]
        if len(store_urls) != len(set(store_urls)):
            raise ValueError('Duplicate store URLs are not allowed')
        
        return platforms

# ### Create account Input Schema
# class CreateAccount(BaseModel):
#     contact_email_address : str
#     @validator('contact_email_address')
#     def validate_email_address(cls, email, **kwargs):
#         try:
#             valid = validate_email(email)
#             return valid.email
#         except EmailNotValidError:
#             raise ValueError('Invalid contact email address provided.')
#     account_name : str
#     account_type : str
#     authenticated_id_key: Union[str, None] = None
#     @validator('account_type')
#     def allowed_account_types(cls, value, **kwargs):
#         allowed = ["startup", "pro", "enterprise"]
#         if value not in allowed:
#             raise ValueError('Invalid account type provided. Allowed permissions : \'startup\', \'pro\' or \'enterprise\'')
#         return value
#     account_currency : str
#     contact_name : str
#     contact_surname : str
#     webhook_url : HttpUrl
#     permissions : List[str]
#     @validator('permissions')
#     def allowed_permissions(cls, value, **kwargs):
#         allowed = ["rec", "churn", "rfm", "seg", "mes"]
#         for v in value:
#             if v not in allowed:
#                 raise ValueError('Invalid permission provided. Allowed permissions : \'rec\', \'churn\' or \'rfm\' or \'seg\' or \'mes\'')
#         return value

### Get accounts internal Input Schema
class GetAccountsInternal(BaseModel):
    account_ids : list