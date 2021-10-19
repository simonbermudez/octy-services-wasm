# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, DateTimeField, \
    ListField, EmbeddedDocument, EmbeddedDocumentField


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
        'db_alias' : 'profile_db',
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