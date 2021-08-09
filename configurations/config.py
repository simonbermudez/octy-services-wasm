from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('CONFIGURATIONS_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('CONFIGURATIONS_CONFIG'))

# Config = {

#     "ENV" : "configurations.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",
#     "CONFIG_EXTENDED_HELP" : "https://octy.ai/docs/getting_started",

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

#          {
#             "QUEUE" : "algo-configs-update-queue",
#             "ROUTING_KEY" : "algo.configs.cmd.update"
#         },
#         {
#             "QUEUE" : "account-configs-update-queue",
#             "ROUTING_KEY" : "account.configs.cmd.update"
#         }


#     ],

#     "OCTY_ALGO_TYPES" : [
#         "rec", "churn"
#     ],

#     "ITEM_SERVICE_CLUSTER_IP" : "http://10.245.172.223:1028"
    
# }
