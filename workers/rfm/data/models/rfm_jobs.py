from pydantic import BaseModel
from typing import Dict, Any


class AccountData(BaseModel):
    account_id : str
    webhook_url : str

class RFMAnalysisJobData(BaseModel):
    bucket : str

class RFMCompleteJobData(BaseModel):
    training_job_id : str
    bucket : str

# ------------------------------

class RFMAnalysisJob(BaseModel):
    account_data : AccountData
    rfm_job_data : RFMAnalysisJobData
    octy_job_id : str

class RFMCompleteJob(BaseModel):
    account_data : AccountData
    rfm_job_data : RFMCompleteJobData
    octy_job_id : str