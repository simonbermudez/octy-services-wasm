from pydantic import BaseModel
from typing import List

class UpdatePastSegementProfilesChild(BaseModel):
    parent_profile : str
    child_profiles : List[str]

class UpdatePastSegementProfiles(BaseModel):
    account_id : str
    profiles : List[UpdatePastSegementProfilesChild]

class DeleteSegments(BaseModel):
    account_id : str
    segment_ids : List[str]