from pydantic import BaseModel
from typing import Dict, Optional, List

class UpdateEventsOwnerChild(BaseModel):
    parent_profile : str
    child_profiles : List[str]

### Update events owner Input Schema
class UpdateEventsOwner(BaseModel):
    account_id : str
    profiles : List[UpdateEventsOwnerChild]

### Delete profile events Input Schema
class DeleteProfiles(BaseModel):
    account_id : str
    profile_id : str