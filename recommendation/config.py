from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('RECOMMENDATION_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('RECOMMENDATION_CONFIG'))

# Config = {

#     "ENV" : "recommendation.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "RECOMENDATIONS_EXTENDED_HELP" : "https://octy.ai/docs/recommendations",

#     "MAX_REC_PREDICTIONS" : 100,

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

#     "DB_ALIAS" : "recommendation_db",

#     "REQUIRED_PERMISSIONS" : ["rec"]
    
# }
