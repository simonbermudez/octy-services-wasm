from pydantic import BaseModel
from typing import Dict


class AccountData(BaseModel):
    account_id : str
    webhook_url : str
    account_type : str
    account_currency : str
    bucket : str
    algorithm_configurations : Dict

class RecCompleteJobData(BaseModel):
    hyperparam_tuning_job_id : str


# ------------------------------

class RecTrainingJob(BaseModel):
    account_data : AccountData
    octy_job_id : str

class RecCompleteJob(BaseModel):
    account_data : AccountData
    job_data : RecCompleteJobData
    octy_job_id : str