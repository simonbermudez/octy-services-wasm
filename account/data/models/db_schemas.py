# module imports
from config import Config

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, BooleanField, \
    DateTimeField, ListField, EmbeddedDocument, EmbeddedDocumentField, ReferenceField, DynamicField
from mongoengine.fields import FloatField

### tbl_accounts schema ---

#Embedded documents
class Keys(EmbeddedDocument):
    public_key = StringField(unique=True)
    secret_key = StringField(unique=True)
class AccountConfigurations(EmbeddedDocument):
    account_type = StringField(required=True)
    account_currency = StringField(required=True)
    contact_name = StringField(required=True)
    contact_surname = StringField(required=True)
    contact_email_address = StringField(required=True)
    webhook_url = StringField(required=True)
    authenticated_id_key = StringField(required=False)
    limits = ListField(default=[{
        "MAX_TOTAL_PROFILES" : Config['MAX_TOTAL_PROFILES'],
        "MAX_TOTAL_ITEMS" : Config['MAX_TOTAL_ITEMS'],
        "MAX_TOTAL_CUSTOM_EVENT_TYPES" : Config['MAX_TOTAL_CUSTOM_EVENT_TYPES'],
        "MAX_TOTAL_EVENTS" : Config['MAX_TOTAL_EVENTS'],
        "MAX_TOTAL_SEGMENT_DEFINITIONS" : Config['MAX_TOTAL_SEGMENT_DEFINITIONS'],
        "MAX_TOTAL_MESSAGE_TEMPLATES" : Config['MAX_TOTAL_MESSAGE_TEMPLATES']
    }]) 


class AlgorithmConfigurations(EmbeddedDocument):
    algorithm_name = StringField(required=True)
    config_json = DynamicField(required=False)
    status = BooleanField(default=True)
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)

    
class ChurnInfo(EmbeddedDocument):
    churn_percentage = FloatField(default=0.0,required=True)
    churn_indicator = StringField(default='stalled',required=True) # positive, negative, stalled
    churn_difference = FloatField(default=0.0,required=True)
    features = ListField(default=[],required=False)

### Parent schema
class tbl_accounts(Document):
    account_id = StringField(primary_key=True)
    account_name = StringField(required=True, unique=True)
    active = BooleanField(default=True)
    bucket = StringField(required=True)
    permissions = ListField(required=True)
    keys = EmbeddedDocumentField(Keys, required=True)
    account_configurations = EmbeddedDocumentField(AccountConfigurations, required=True)
    algorithm_configurations = ListField(EmbeddedDocumentField(AlgorithmConfigurations), required=True)
    churn_info = EmbeddedDocumentField(ChurnInfo, required=True)
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)
    last_updated_action = StringField(null=True)

### tbl_segments [one to many]
class tbl_segments(Document):
    segment_id = StringField(primary_key=True)
    segment_name = StringField(required=True) # Do not allow duplicates (per account)
    segment_type = IntField(required=True) #live | past
    segment_sub_type = IntField(required=True) #1,2,3,4
    segment_timeframe = IntField(required=True, default=0) #Not required for LIVE segmentation.
    event_sequence = DynamicField(required=False)
    profile_property_name = StringField(null=True)
    profile_property_value = StringField(null=True)
    octy_job_id = StringField(null=True, required=False) #[one to squillions] ref to octy job (if past segment only!)
    status = StringField(default='active') #updated to 'pending_deletion' if user requests it to be deleted.
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)

# ---

### tbl_notifications schema
class tbl_notifications(Document):
    notification_id = StringField(required=True, unique=True)
    account = ReferenceField('tbl_accounts') #[one to squillions]
    notification_content = StringField()
    notification_type = StringField(required=True)  # webhook, email
    destination = StringField(required=True)
    did_succeed = BooleanField(default=True)
    created_at = DateTimeField(default=dt.now)


### tbl_failed_auth_attempts schema
class tbl_failed_auth_attempts(Document):
    public_key = StringField(required=True)
    user_agent = StringField()
    server_name = StringField()
    server_port = IntField()
    request_type = StringField()
    created_at = DateTimeField(default=dt.now)



