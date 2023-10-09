from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('ITEMS_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('ITEMS_CONFIG'))  