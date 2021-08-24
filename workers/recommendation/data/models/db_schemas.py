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

class Recommendations(EmbeddedDocument):
    score = FloatField(required=True)
    item_id = StringField(required=True)

class tbl_recommendations_cache(Document):
    account_id = StringField(required=True)
    training_job_id = StringField(required=True)
    profile_id = StringField(required=True)
    recommendations = ListField(EmbeddedDocumentField(Recommendations))
    created_at = DateTimeField(default=dt.now)