from pydantic import BaseModel, validator
from typing import List
from config import Config

### Get recommendations schema
class GetRecomendations(BaseModel):
    profile_ids : List[str]
    @validator('profile_ids')
    def length(cls, v):
        if len(v) > Config['MAX_REC_PREDICTIONS']:
            raise ValueError('You can only generate up to 100 item recommendations per request.')
        return v

### GetRecomendations Internal schema
class GetRecomendationsInternal(BaseModel):
    account_id : str
    profile_ids : List[str]