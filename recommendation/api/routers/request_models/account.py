from pydantic import BaseModel
from typing import List, Dict, Any

### Session Account schema
class Account(BaseModel):
    account_id : str
    account_name : str
    bucket : str
    permissions : List
    account_configurations : Dict
    algorithm_configurations : List
    churn_info : Dict
    created_at : Any