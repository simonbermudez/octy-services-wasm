# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField,DateTimeField, ListField, DynamicField

### tbl_custom_event_types schema ---
class tbl_custom_event_types(Document):
    event_type_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    event_type = StringField(required=True, unique_with=['account_id'])
    event_properties = ListField(default=[])
    created_at = DateTimeField(default=dt.now)


### tbl_event_instances schema ---
class tbl_event_instances(Document):
    event_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    profile_id = StringField(required=True) #[one to squillions]
    event_type_id = StringField(required=True) #"<Custom event id | system event type>",
    event_type = StringField(required=True)
    event_properties = DynamicField() # client defined event properties object
    created_at = DateTimeField(default=dt.now)