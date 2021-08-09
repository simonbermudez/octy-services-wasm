from pydantic import BaseModel
from typing import Dict, Optional, List

### Create octy job Input Schema

class JobMeta(BaseModel):
    desired_runs : int
    time_interval : int # minutes
    fail_threshold : int
class CreateOctyJob(BaseModel):
    account_id : str
    alt_dentifier : Optional[str]
    job_type : str
    job_meta : JobMeta
    job_data : Optional[Dict]

### Delete octy job Input Schema
class DeleteOctyJob(BaseModel):
    account_id : str
    octy_job_ids : Optional[List[str]]
    alt_identifiers : Optional[List[str]]