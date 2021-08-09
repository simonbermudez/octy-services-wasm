from utils.utils import base64_decode
import os
import json

#os.environ.get('CLOUDAMQP_URL', 'amqp://guest:guest@localhost:5672/%2f')

try:
    Config = json.loads(base64_decode(os.environ.get('ACCOUNT_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('ACCOUNT_CONFIG'))

# Config = {

#     "ENV" : "account.development",
    
#     "SUPPORT_EMAIL" : "support@octy.ai",
#     "DOCS_ROOT_URL" : "https://octy.ai/docs",
    
#     "FAILED_AUTH_ATTEMPT_LIMIT" : 20,

#     "MAX_PAGINATION_RESULT" : 200,
#     "MAX_CREATE_PROFILES" : 100,
#     "MAX_UPDATE_DELETE_PROFILES" : 100,
#     "MAX_CREATE_ITEMS" : 100,
#     "MAX_UPDATE_DELETE_ITEMS" : 100,
#     "MAX_SET_EVENT_TYPES" : 100,
#     "MAX_DELETE_EVENT_TYPES" : 100,
#     "MAX_CREATE_EVENTS" : 100,
#     "MAX_DELETE_SEGMENTS" : 100,
#     "MESSAGE_GEN_LIMIT" : 20,
#     "MAX_REC_PROFILE_IDS" : 20, 
#     "MAX_TOTAL_PROFILES" : 10000,
#     "MAX_TOTAL_ITEMS" : 150,
#     "MAX_TOTAL_CUSTOM_EVENT_TYPES" : 100,
#     "MAX_TOTAL_EVENTS" : 1000000,
#     "MAX_TOTAL_SEGMENT_DEFINITIONS" : 25,
#     "MAX_TOTAL_MESSAGE_TEMPLATES" : 50,

#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "algo-configs-update-queue",
#             "ROUTING_KEY" : "algo.configs.cmd.update"
#         },
#         {
#             "QUEUE" : "account-configs-update-queue",
#             "ROUTING_KEY" : "account.configs.cmd.update"
#         },
#         {
#             "QUEUE" : "churn-info-update-queue",
#             "ROUTING_KEY" : "churn.info.cmd.update"
#         }

#     ],

#     "AMQP_PUBLISHERS" : [
#         {
#             "QUEUE" : "octy-job-create-queue",
#             "ROUTING_KEY" : "octy.job.cmd.create"
#         }
#     ],

#     "ML_JOBS" : ["rec", "churn", "rfm"],
    
#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",

#     "ERROR_TEMPLATE" : {
#         "request_meta" : { 
#             "request_status" : "Failure" , 
#             "message" : ""
#         },
#     "error" : {
#             "code" : 0,
#             "reason" : "",
#             "errors" : []
#         }
#     },

#     "SENTRY_URL" : "https://e4b80290888a4267a2224efc6dbed258@o324132.ingest.sentry.io/1826169",

#     "DB_ALIAS" : "account_db",

#     "AWS_REGION" : "eu-west-2",
#     "AWS_ALLOWED_IP" : "176.253.204.11",
#     "AWS_SERVER_SIDE_ENCRYPTION" : "AES256",
    
#      "BUCKET_REQUIRED_DIRS" : [
#         "resources/raw_data/profiles",
#         "resources/raw_data/items",
#         "resources/training_job_data/recommendations_data",
#         "resources/training_job_data/churn_prediction_data",
#         "resources/training_job_data/rfm_data",
#         "resources/training_job_data/ltv_data",
#         "models/recommendation_models",
#         "models/churn_prediction_models",
#          "models/rfm_dataframes"
#         ]      
# }

'''
S3 Bucket directories reference: 

#resources::
#resources/raw_data/customer_profiles/{filename}

#resources/raw_data/items/{filename}

#resources/training_job_data/recommendations_data/{training_job_id}/{filename}

#resources/training_job_data/churn_prediction_data/{training_job_id}/{filename}

#resources/training_job_data/rfm_data/{training_job_id}/{filename}

#resources/training_job_data/ltv_data/{training_job_id}/{filename}

#message templates::
#resources/templates/{file_id}

#models/dataframes::
#models/recommendation_models/{training_job_id}/output/{model}

#models/churn_prediction_models/{training_job_id}/output/{model}

#models/rfm_dataframes/{training_job_id}/output/{dataframe}


Not implemented!
#models/ltv_prediction_models/{training_job_id}/output/{model}
'''