from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('MESSAGING_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('MESSAGING_CONFIG'))

# Config = {

#     "ENV" : "messaging.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "MESSAGING_EXTENDED_HELP" : "https://octy.ai/docs/messaging",

#     "MAX_CREATE_TEMPLATES" : 100,
#     "MAX_UPDATE_DELETE_TEMPLATES" : 100,
#     "MESSAGE_GEN_LIMIT" : 100,
    
#     "REC_SERVICE_CLUSTER_IP" : "https://sandbox.api.octy.ai",
#     "ITEM_SERVICE_CLUSTER_IP" : "https://sandbox.api.octy.ai",

#     "ERROR_TEMPLATE" : {
#         "request_meta" : { 
#             "request_status" : "Failure" , 
#             "message" : ""
#         },
#         "error" : {
#             "code" : 0,
#             "reason" : "",
#             "errors" : []
#         }
#     },

#     "SENTRY_URL" : "https://e4b80290888a4267a2224efc6dbed258@o324132.ingest.sentry.io/1826169",

#     "DB_ALIAS" : "template_db",

#     "REQUIRED_PERMISSIONS" : ["mes"],
    
# }
