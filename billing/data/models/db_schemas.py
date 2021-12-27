# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, DateTimeField

### tbl_items schema ---
class tbl_billable_units(Document):
    account_id = StringField(required=True)
    account_type = StringField(required=True)
    process_name = StringField(required=True)
    unit_type = StringField(required=True)
    metric = StringField(required=True)
    quantity = IntField(required=True)
    cost_per_unit = IntField(required=True)
    total_cost = IntField(required=True)
    currency = StringField(required=True)
    created_at = DateTimeField(default=dt.now)
    meta = {
        'index_background': True,
        'indexes': [
            {
                'fields': ['account_id'],
                'name': 'account_id'
            },
            {
                'fields': ['account_type'],
                'name': 'account_type'
            },
            {
                'fields': ['account_id', 'unit_type'],
                'name': 'account_id_unit_type'
            },
            {
                'fields': ['account_id', 'unit_type', 'metric'],
                'name': 'account_id_unit_type_metric'
            },
            {
                'fields': ['account_id', 'process_name'],
                'name': 'account_id_process_name'
            },
            {
                'fields': ['process_name'],
                'name': 'process_name'
            }
        ]
    }