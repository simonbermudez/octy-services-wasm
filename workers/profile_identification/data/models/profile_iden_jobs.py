from pydantic import BaseModel
from typing import Dict, Any


class AccountData(BaseModel):
    account_id : str
    webhook_url : str

class ProfileIdenJobData(BaseModel):
    authenticated_id_key : str

# ------------------------------

class ProfileIdenJob(BaseModel):
    account_data : AccountData
    profile_iden_job_data : ProfileIdenJobData
    octy_job_id : str