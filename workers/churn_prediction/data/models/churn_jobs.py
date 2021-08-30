from pydantic import BaseModel
from typing import Dict, Any


class AccountData(BaseModel):
    account_id : str
    webhook_url : str

class ChurnTrainingJobData(BaseModel):
    bucket : str
    algorithm_configurations : Dict

class ChurnCompleteJobData(BaseModel):
    hyperparam_tuning_job_id : str
    previous_churn_percentage : Any # int or float
    bucket : str
    algorithm_configurations : Dict

# ------------------------------

class ChurnTrainingJob(BaseModel):
    account_data : AccountData
    churn_job_data : ChurnTrainingJobData
    octy_job_id : str

class ChurnCompleteJob(BaseModel):
    account_data : AccountData
    churn_job_data : ChurnCompleteJobData
    octy_job_id : str