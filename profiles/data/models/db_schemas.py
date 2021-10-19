# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, BooleanField, \
    DateTimeField, DynamicField, ListField, EmbeddedDocument, EmbeddedDocumentField

### tbl_profiles schema ---

### one to many
class SegmentTags(EmbeddedDocument):
    segment_id = StringField(required=True) #[one to squillions]
    segment_tag = StringField(required=True)
    status = StringField(default='active') #Pending relates to live segmentation, Inactive relates to past segmentation.
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)

### Parent schema
class tbl_profiles(Document):
    profile_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    customer_id = StringField(required=True, unique_with=['account_id'])
    profile_data = DynamicField() #store JSON object on customer data
    platform_info = DynamicField() #store JSON object on customers platform useage data: eg. iPhone, web browser etc.
    rfm_score = IntField(null=True)
    rfm_segment_desc = StringField(null=True)
    churn_probability = StringField(null=True)
    has_charged = BooleanField(default=False)
    ltv_prediction = IntField(null=True)
    current_ltv = IntField(null=True)
    segment_tags = ListField(EmbeddedDocumentField(SegmentTags),required=False, default=[])
    status = StringField(default='active') # set to 'churned' if event type of churn occurs for this user.
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'status', 'profile_id'],
                'name': 'account_id_status_profile_id'
            },
            {
                'fields': ['account_id', 'status'],
                'name': 'account_id_status'
            },
            {
                'fields': ['account_id'],
                'name': 'account_id'
            }
        ]
    }

### tbl_merged_profiles schema ---

class MergedProfiles(EmbeddedDocument):
    profile_id = StringField(required=True)
    customer_id = StringField(required=True)

class tbl_merged_profiles(Document):
    account_id = StringField(required=True)
    merged_profiles = ListField(EmbeddedDocumentField(MergedProfiles),required=False, default=[])
    parent_profile_id = StringField(required=False)
    parent_customer_id = StringField(required=False)
    authenticated_id_key = StringField(required=True)
    authenticated_id_value = StringField(required=True)
    created_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'parent_profile_id'],
                'name': 'account_id_parent_profile_id'
            },
            {
                'fields': ['account_id', 'merged_profiles.profile_id'],
                'name': 'account_id_profile_id'
            },
            {
                'fields': ['account_id', 'merged_profiles.customer_id'],
                'name': 'account_id_customer_id'
            }
        ]
    }