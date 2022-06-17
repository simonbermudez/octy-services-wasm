# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, BooleanField, \
    DateTimeField, DynamicField, ListField, EmbeddedDocument, EmbeddedDocumentField

### tbl_templates schema ---

class tbl_templates(Document):
    template_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    friendly_name = StringField(required=True, unique_with=['account_id'])
    template_type = StringField(required=True)
    title = StringField(required=True)
    content = StringField(required=True)
    default_values = DynamicField(required=False)
    metadata = DynamicField(required=False, default={})
    status = StringField(default='active')
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)
    meta = {
        'db_alias': 'template_db',
        'collection': 'tbl_templates',
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id', 'status'],
                'name': 'account_id_status'
            }
        ]
    }

### tbl_currency_rates schema ---
class tbl_currency_rates(Document):
    rates = ListField()
    created_at = DateTimeField()
    meta = {
        'db_alias': 'currency_rates_db',
        'collection': 'tbl_currency_rates'
    }