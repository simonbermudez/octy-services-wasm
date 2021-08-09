# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, DateTimeField

### tbl_items schema ---
class tbl_items(Document):
    item_id = StringField(unique_with=['account_id'])
    account_id = StringField(required=True)
    item_category = StringField(required=True)
    item_name = StringField(required=True)
    item_description = StringField(required=True)
    item_price = IntField(required=True)
    event_type = StringField(required=True)
    status = StringField(default='active') # active, inactive
    created_at = DateTimeField(default=dt.now)
    updated_at = DateTimeField(null=True)