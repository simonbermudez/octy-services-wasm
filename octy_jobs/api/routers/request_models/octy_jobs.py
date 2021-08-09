from pydantic import BaseModel
from typing import List, Dict


### Octyjob Callback Input Schema
class OctyJobCallBack(BaseModel):
    account_id : str
    octy_job_id : str
    message : str
    status : str
