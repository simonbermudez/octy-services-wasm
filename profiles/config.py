from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('PROFILES_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('PROFILES_CONFIG'))

# Config = {

#     "ENV" : "profiles.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "ERRORS_OVERVIEW_EXTENDED_HELP" : "https://octy.ai/docs/api#errors",
#     "ERRORS_OVERVIEW_EXTENDED_HELP" : "https://octy.ai/docs/api#errors",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "PROFILES_EXTENDED_HELP" : "https://octy.ai/docs/api#ProfileObject",

#     "MAX_CREATE_PROFILES" : 100,
#     "MAX_UPDATE_DELETE_PROFILES" : 100,

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

#     "SENTRY_URL" : "https://1ef5498a6b6f4de2b60a43268c7617db@o4503948099125248.ingest.sentry.io/4503977742041088",

#     "DB_ALIAS" : "profile_db",


#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_PUBLISHERS" : [
#         {
#             "QUEUE" : "profile-events-delete-queue",
#             "ROUTING_KEY" : "events.cmd.delete"
#         }
#     ],
#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "profiles-update-queue",
#             "ROUTING_KEY" : "profiles.cmd.update"
#         },
#         {
#             "QUEUE" : "segment-tags-update-delete-queue",
#             "ROUTING_KEY" : "segment.tags.cmd.update.delete"
#         },
#         {
#             "QUEUE" : "grouped-segmentation-operations-queue",
#             "ROUTING_KEY" : "grouped.segmentation.operations.cmd"
#         }
#     ]

# }
