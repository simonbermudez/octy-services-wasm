from pydantic import BaseModel

class DeleteAccountBilling(BaseModel):
    account_id : str
