from pydantic import BaseModel
from typing import List, Optional

class SegmentId(BaseModel):
    segment_id : str
class SegmentIDUpdateDelete(BaseModel):
    account_id : str
    action : str
    segment_ids : List[SegmentId]

class GroupedSegmentationDatabaseOperations(BaseModel): 
    account_id : str
    operations : List[dict]