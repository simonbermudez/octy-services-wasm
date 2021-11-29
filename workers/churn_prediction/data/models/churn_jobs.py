from pydantic import BaseModel
from typing import Dict, Any, Optional


class AccountData(BaseModel):
    account_id : str
    webhook_url : Optional[str]
    bucket : str
    churn_percentage : Any # int or float
    algorithm_configurations : Dict

class ChurnCompleteJobData(BaseModel):
    hyperparam_tuning_job_id : str

# ------------------------------

class ChurnTrainingJob(BaseModel):
    account_data : AccountData
    octy_job_id : str

class ChurnCompleteJob(BaseModel):
    account_data : AccountData
    job_data : ChurnCompleteJobData
    octy_job_id : str