# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, DateTimeField, ListField, EmbeddedDocumentField, EmbeddedDocument, DynamicField

### tbl_octy_jobs schema ---
class RequiredConfigs(EmbeddedDocument):
    account_attributes = ListField(StringField(),default=[])
    algorithm_configuration_idxs = ListField(IntField(),default=[])

class JobMeta(EmbeddedDocument):
    job_type = StringField(required=True)
    amqp_routing_key = StringField(required=True)
    required_permissions = ListField(StringField(),default=[])
    required_configurations = EmbeddedDocumentField(RequiredConfigs)
    desired_runs = IntField(default=0)
    successful_runs = IntField(default=0)
    failed_runs = IntField(default=0)
    last_run = DateTimeField(null=True)
    time_interval = IntField(required=True) # minutes
    fail_threshold = IntField(required=True)
    status = StringField(default='pending')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)
    last_updated_action = StringField(null=True)


class tbl_octy_jobs(Document):
    octy_job_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    alt_dentifier = StringField(required=False,null=True)
    job_meta = EmbeddedDocumentField(JobMeta)
    job_data = DynamicField(required=False)
  