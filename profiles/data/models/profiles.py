from pydantic import BaseModel, validator
from typing import List, Dict, Optional

### Update customer profiles Input Schema
class SegmentTags(BaseModel):
    segment_id : str
    segment_tag : str
    status : Optional[str]
class UpdateProfilesChild(BaseModel):
    profile_id : str
    customer_id : Optional[str]
    profile_data : Optional[Dict]
    platform_info : Optional[Dict]
    has_charged : Optional[bool]
    status: Optional[str]
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
    account_id : str
    profiles : List[UpdateProfilesChild]