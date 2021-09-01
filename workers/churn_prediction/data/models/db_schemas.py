# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, DateTimeField, DynamicField

class tbl_training_dataset_cache(Document):
    account_id = StringField(required=True)
    hyperparam_tuning_job_id = StringField(required=True)
    row_data = DynamicField()
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'hyperparam_tuning_job_id'],
                'name': 'account_id_job_id'
            }
        ]
    }

class tbl_training_jobs(Document):
    training_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    meta_data = DynamicField(required=False)
    model_meta_data = DynamicField(required=False)
    status = StringField(default='in_progress')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(default=dt.now)

class tbl_hparam_tuning_jobs(Document):
    hyperparam_tuning_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    meta_data = DynamicField(required=False)
    best_model_meta_data = DynamicField(required=False)
    best_model_training_job_id = StringField(required=False)
    status = StringField(default='in_progress')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'status', 'hyperparam_tuning_job_id'],
                'name': 'account_id_status_job_id'
            },
            {
                'fields': ['account_id', 'status'],
                'name': 'account_id_status'
            }
        ]
    }