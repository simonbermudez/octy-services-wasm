from pydantic import BaseModel
from typing import List

### Delete recommendations cache Input Schema
class DeleteRecCache(BaseModel):
    account_id : str
    profiles : List[str]