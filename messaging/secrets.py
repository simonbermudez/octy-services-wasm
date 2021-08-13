from utils.utils import base64_decode
import os
import json

try:
    Secrets = json.loads(base64_decode(os.environ.get('MESSAGING_SECRETS')))
except TypeError:
    Secrets = base64_decode(os.environ.get('MESSAGING_SECRETS'))
