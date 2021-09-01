# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, DateTimeField, DynamicField

class tbl_training_jobs(Document):
    training_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    meta_data = DynamicField(required=False)
    model_meta_data = DynamicField(required=False)
    status = StringField(default='in_progress')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'status', 'training_job_id'],
                'name': 'account_id_status_training_job_id '
            }
        ]
    }