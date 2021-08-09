from utils.utils import base64_decode
import os
import json

#os.environ.get('CLOUDAMQP_URL', 'amqp://guest:guest@localhost:5672/%2f')

try:
    Config = json.loads(base64_decode(os.environ.get('ITEMS_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('ITEMS_CONFIG'))


# Config = {

#     "ENV" : "items.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "ITEMS_EXTENDED_HELP" : "https://octy.ai/docs/api#ItemObject",

#     "MAX_CREATE_ITEMS" : 100,
#     "MAX_UPDATE_DELETE_ITEMS" : 100,

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

#     "DB_ALIAS" : "item_db",

#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_PUBLISHERS" : [

#          {
#             "QUEUE" : "algo-configs-update-queue",
#             "ROUTING_KEY" : "algo.configs.cmd.update"
#         }

#     ],
# }
