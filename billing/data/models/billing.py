from pydantic import BaseModel, validator
from typing import List, Dict, Optional

### Billable units Input Schema
class UnitsChild(BaseModel):
    unit_type : str
    metric : str
    process_name : str
    quantity : int
    account_id : str
    account_currency : str
    account_type : str

class BillableUnits(BaseModel):
    units : List[UnitsChild]