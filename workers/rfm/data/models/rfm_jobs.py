from pydantic import BaseModel
from typing import Optional


class AccountData(BaseModel):
    account_id : str
    webhook_url : Optional[str]
    bucket : str

class RFMCompleteJobData(BaseModel):
    training_job_id : str

# ------------------------------

class RFMAnalysisJob(BaseModel):
    account_data : AccountData
    octy_job_id : str

class RFMCompleteJob(BaseModel):
    account_data : AccountData
    job_data : RFMCompleteJobData
    octy_job_id : str