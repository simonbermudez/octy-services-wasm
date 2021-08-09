from pydantic import BaseModel
from typing import Dict, Optional, List


class AccountData(BaseModel):
    account_id : str
    webhook_url : str

class RecTrainingJobData(BaseModel):
    bucket : str
    algorithm_configurations : Dict

class RecCompleteJobData(BaseModel):
    training_job_id : str
    bucket : str
    algorithm_configurations : Dict

# ------------------------------

class RecTrainingJob(BaseModel):
    account_data : AccountData
    rec_job_data : RecTrainingJobData
    octy_job_id : str

class RecCompleteJob(BaseModel):
    account_data : AccountData
    rec_job_data : RecCompleteJobData
    octy_job_id : str