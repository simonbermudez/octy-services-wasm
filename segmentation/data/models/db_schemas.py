# module imports

# python imports
from datetime import datetime as dt

# external imports
from mongoengine import Document, StringField, IntField, DateTimeField, ListField, EmbeddedDocumentField, EmbeddedDocument, DynamicField

### tbl_segments schema ---
class EventSequence(EmbeddedDocument):
    # event = StringField(required=True)
    event_type = StringField(required=True)
    exp_timeframe = IntField(required=True)
    action_inaction = StringField(required=True)
    event_properties = DynamicField(required=False, default=None)

class tbl_segments(Document):
    segment_id = StringField(primary_key=True)
    account_id = StringField(required=True)
    segment_name = StringField(required=True, unique_with=['account_id']) # Do not allow duplicates
    segment_type = StringField(required=True) #live | past
    segment_sub_type = IntField(required=True) #1,2,3,4
    segment_timeframe = IntField(default=0) #Not required for LIVE segmentation.
    event_sequence = ListField(EmbeddedDocumentField(EventSequence), required=True) #raw list
    profile_property_name = StringField(null=True)
    profile_property_value = DynamicField(null=True)
    profile_ids = ListField(required=False, default=[]) # Profile IDs that met this segments criteria on the last run. PAST SEGMENTATION
    status = StringField(default='active') #updated to 'pending_deletion' if user requests it to be deleted.
    created_at = DateTimeField(default=dt.now)