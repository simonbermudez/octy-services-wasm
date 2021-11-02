from pydantic import BaseModel
from typing import Optional, Dict, List, Any


class AccountData(BaseModel):
    account_id : str
    webhook_url : str

class SegmentData(BaseModel):
    segmentation_type : str
    segment_id : Optional[str]

class SegmentTags(BaseModel):
    segment_id : str
    segment_tag : str
    status : Optional[str]

class ProfileData(BaseModel):
    profile_id : str
    customer_id : Optional[str]
    profile_data : Optional[Dict]
    platform_info : Optional[Dict]
    has_charged : Optional[bool]
    status: Optional[str]
    rfm_score : Optional[int]
    rfm_segment_desc : Optional[str]
    churn_probability : Optional[str]
    ltv_prediction : Optional[int]
    current_ltv : Optional[int]
    segment_tags : Optional[List[SegmentTags]]

class EventData(BaseModel):
    event_id : Optional[str]
    event_type : Optional[str]
    event_properties : Optional[Dict]
    created_at : Optional[Any]
    profile : ProfileData
    event_timeframe : Optional[int]


class PastJobData(BaseModel):
    segment_data : SegmentData

class LiveJobData(BaseModel):
    segment_data : SegmentData
    event_data : EventData
    validation_job : bool
    live_octy_job_id : Optional[str]

# ------------------------------

class PastSegmentationJob(BaseModel):
    account_data : AccountData
    job_data : PastJobData
    octy_job_id : str

class LiveSegmentationJob(BaseModel):
    account_data : AccountData
    job_data : LiveJobData
    octy_job_id : str