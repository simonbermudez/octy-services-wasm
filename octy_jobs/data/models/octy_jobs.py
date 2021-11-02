from pydantic import BaseModel
from typing import Dict, Optional, List

### Create octy job Input Schema

class RequiredConfigs(BaseModel):
    account_attributes : List[str]
    algorithm_configuration_idxs : List[int]

class JobMeta(BaseModel):
    job_type : str
    amqp_routing_key : str
    required_permissions : List[str]
    required_configurations : RequiredConfigs
    desired_runs : int
    time_interval : int # minutes
    fail_threshold : int

class CreateOctyJob(BaseModel):
    account_id : str
    alt_dentifier : Optional[str]
    job_meta : JobMeta
    job_data : Optional[Dict]

### Delete octy job Input Schema
class DeleteOctyJob(BaseModel):
    account_id : str
    octy_job_ids : Optional[List[str]]
    alt_identifiers : Optional[List[str]]