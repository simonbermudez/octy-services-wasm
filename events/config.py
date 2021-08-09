from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('EVENTS_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('EVENTS_CONFIG'))


# Config = {

#     "ENV" : "events.development",
#     "SUPPORT_EMAIL" : "support@octy.ai",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "CUSTOM_EVENTS_EXTENDED_HELP" : "https://octy.ai/docs/api#CustomEventObject",
#     "EVENTS_EXTENDED_HELP" : "https://octy.ai/docs/creating_resources#Creating%20events",

#     "MAX_CREATE_EVENT_TYPES" : 100,
#     "MAX_DELETE_EVENT_TYPES" : 100,
#     "MAX_CREATE_EVENTS" : 100,

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

#     "REQUIRED_PERMISSIONS" : ["seg"],

#     "SYSTEM_EVENT_TYPES" : ["charged","churned","complaint"],

#     "PROFILE_SERVICE_CLUSTER_IP" : "https://sandbox.api.octy.ai",
#     "SEGMENTATION_SERVICE_CLUSTER_IP" : "https://sandbox.api.octy.ai",

#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_PUBLISHERS" : [

#          {
#             "QUEUE" : "octy-job-create-queue",
#             "ROUTING_KEY" : "octy.job.cmd.create"
#         },
#         {
#             "QUEUE" : "profiles-update-queue",
#             "ROUTING_KEY" : "profiles.cmd.update"
#         }
#     ],
#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "profile-events-delete-queue",
#             "ROUTING_KEY" : "events.cmd.delete"
#         }
#     ]
# }

# "PROFILE_SERVICE_CLUSTER_IP" : "http://0.0.0.0:8080",
#"PROFILE_SERVICE_CLUSTER_IP" : "http://10.245.175.16:1027",