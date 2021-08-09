from pydantic import BaseModel
from typing import Dict, Optional, List

### Delete profile events Input Schema
class DeleteProfiles(BaseModel):
    account_id : str
    profile_id : str