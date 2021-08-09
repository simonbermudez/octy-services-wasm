from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('OCTY_JOB_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('OCTY_JOB_CONFIG'))

# Config = {

#     "ENV" : "octy_jobs.development",

#     "AUTH_EXTENDED_HELP" : "https://octy.ai/docs/api#authentication",
#     "INVALID_JSON_EXTENDED_HELP" : "https://octy.ai/docs/invalid_json",
#     "SERVER_ERROR_EXTENDED_HELP" : "https://octy.ai/docs/server_error",
#     "RATE_LIMIT_EXTENDED_HELP" : "https://octy.ai/docs/api#limits",

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

#     "DB_ALIAS" : "octy_job_db",

#     "ACCOUNT_SERVICE_CLUSTER_IP" : "https://sandbox.api.octy.ai",

#     "AMQP_URL" : "amqps://junotddp:WsfkqDAXlZcIJqZ2zDN0ghEQT-Bnqa9i@hippo.rmq2.cloudamqp.com/junotddp",
#     "EXCHANGE" : "octy-services",

#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "octy-job-create-queue",
#             "ROUTING_KEY" : "octy.job.cmd.create"
#         },
#         {
#             "QUEUE" : "octy-job-delete-queue",
#             "ROUTING_KEY" : "octy.job.cmd.delete"
#         }
#     ],
#     "AMQP_PUBLISHERS" : [

#         {
#             "QUEUE" : "live-segmentation-run-queue",
#             "ROUTING_KEY" : "live.segmentation.cmd.run"
#         },
#         {
#             "QUEUE" : "past-segmentation-run-queue",
#             "ROUTING_KEY" : "past.segmentation.cmd.run"
#         },
#         {
#             "QUEUE" : "rec-training-run-queue",
#             "ROUTING_KEY" : "rec.training.cmd.run"
#         },
#         {
#             "QUEUE" : "rec-training-complete-queue",
#             "ROUTING_KEY" : "rec.training.complete.cmd.run"
#         },
#         {
#             "QUEUE" : "churn-training-run-queue",
#             "ROUTING_KEY" : "churn.training.cmd.run"
#         },
#         {
#             "QUEUE" : "churn-training-complete-queue",
#             "ROUTING_KEY" : "churn.training.complete.cmd.run"
#         },
#         {
#             "QUEUE" : "rfm-training-run-queue",
#             "ROUTING_KEY" : "rfm.training.cmd.run"
#         },
#         {
#             "QUEUE" : "rfm-training-complete-queue",
#             "ROUTING_KEY" : "rfm.training.complete.cmd.run"
#         }
#     ]
# }
