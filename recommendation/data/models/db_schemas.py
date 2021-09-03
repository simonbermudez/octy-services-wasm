# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, \
    DateTimeField, ListField, EmbeddedDocumentField, EmbeddedDocument, DynamicField, FloatField

class lfm(EmbeddedDocument):
    lfm_idx = IntField(required=True)
    type_ = StringField(required=True)
    res_id = StringField(required=True)

class tbl_training_jobs(Document):
    training_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    meta_data = DynamicField(required=False)
    model_meta_data = DynamicField(required=False)
    lfm_idxs = ListField(EmbeddedDocumentField(lfm))
    status = StringField(default='in_progress')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(default=dt.now)

class tbl_hparam_tuning_jobs(Document):
    hyperparam_tuning_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    meta_data = DynamicField(required=False)
    best_model_meta_data = DynamicField(required=False)
    best_model_training_job_id = StringField(required=False)
    lfm_idxs = ListField(EmbeddedDocumentField(lfm))
    status = StringField(default='in_progress')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'status'],
                'name': 'account_id_status'
            },
            {
                'fields': ['account_id', 'status', 'hyperparam_tuning_job_id'],
                'name': 'account_id_status_hyperparam_tuning_job_id'
            }
        ]
    }

class Recommendations(EmbeddedDocument):
    score = FloatField(required=True)
    item_id = StringField(required=True)

class tbl_recommendations_cache(Document):
    account_id = StringField(required=True)
    training_job_id = StringField(required=True)
    profile_id = StringField(required=True)
    recommendations = ListField(EmbeddedDocumentField(Recommendations))
    created_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id','training_job_id', 'profile_id'],
                'name': 'account_id_training_job_id_profile_id'
            }
        ]
    }