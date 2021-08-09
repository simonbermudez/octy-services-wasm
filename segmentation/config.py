from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('SEGMENTATION_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('SEGMENTATION_CONFIG'))
    
# Config = {

#     "ENV" : "segmentation.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "SEGMENTATION_EXTENDED_HELP" : "https://octy.ai/docs/segmentation",

#     "MAX_DELETE_SEGMENTS" : 100,

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

#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_PUBLISHERS" : [

#         {
#             "QUEUE" : "octy-job-create-queue",
#             "ROUTING_KEY" : "octy.job.cmd.create"
#         },
#         {
#             "QUEUE" : "octy-job-delete-queue",
#             "ROUTING_KEY" : "octy.job.cmd.delete"
#         },
#         {
#             "QUEUE" : "segment-tags-update-delete-queue",
#             "ROUTING_KEY" : "segment.tags.cmd.update.delete"
#         }
#     ],


#     "REQUIRED_PERMISSIONS" : ["seg"],

#     "EVENT_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
    
#     "PAST_SEGMENTATION_JOB_INTERVAL" : 1440,

#     "SYSTEM_EVENT_TYPES_MAP" : [
#         {
#             "event_type" : "charged",
#             "event_properties" : {
#                 "payment_method" : "string",
#                 "item_id" : "string"
#             }
#         },
#         {
#             "event_type" : "complaint",
#             "event_properties" : {
#                 "channel" : "string"
#             }
#         },
#         {
#             "event_type" : "churned",
#             "event_properties" : None
#         }
#     ]
# }

# '''
#     "AMQP_CONSUMERS" : [
#         {
#             "QUEUE" : "segments-delete-queue",
#             "ROUTING_KEY" : "segments.cmd.delete"
#         }
#     ],

# '''