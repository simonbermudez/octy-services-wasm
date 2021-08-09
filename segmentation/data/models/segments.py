from pydantic import BaseModel
from typing import List

class DeleteSegments(BaseModel):
    account_id : str
    segment_ids : List[str]