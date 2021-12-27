from pydantic import BaseModel


class AccountData(BaseModel):
    account_id : str
    webhook_url : str
    account_type : str
    account_currency : str
    authenticated_id_key : str
    

# ------------------------------

class ProfileIdenJob(BaseModel):
    account_data : AccountData
    octy_job_id : str