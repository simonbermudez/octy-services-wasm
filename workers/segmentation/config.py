from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('SEGMENTATION_WORKER_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('SEGMENTATION_WORKER_CONFIG'))

# "SENTRY_URL" : "https://e4b80290888a4267a2224efc6dbed258@o324132.ingest.sentry.io/1826169",


# Config = {
    
#     "ENV" : "segmentation.worker.development",
#     "SENTRY_URL" : "https://e4b80290888a4267a2224efc6dbed258@o324132.ingest.sentry.io/1826169",

#     "PROFILE_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "EVENT_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "SEGMENTATION_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "OCTY_JOB_SERVICE_CLUSTER_IP" : "https://api.octy.ai",

#     "AMQP_URL" : "amqps://kcditigk:jckigtvlFqNxH652hdYIqRPQH7kPuS0n@brilliant-grey-impala.rmq2.cloudamqp.com/kcditigk",
#     "EXCHANGE" : "octy-services",

#     "AMQP_PUBLISHERS" : [

#         {
#             "QUEUE" : "octy-job-delete-queue",
#             "ROUTING_KEY" : "octy.job.cmd.delete"
#         },
#         {
#             "QUEUE" : "octy-job-create-queue",
#             "ROUTING_KEY" : "octy.job.cmd.create"
#         },
#         {
#             "QUEUE" : "grouped-segmentation-operations-queue",
#             "ROUTING_KEY" : "grouped.segmentation.operations.cmd"
#         }
#     ],
#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "past-segmentation-run-queue",
#             "ROUTING_KEY" : "past.segmentation.cmd.run"
#         },
#         {
#             "QUEUE" : "live-segmentation-run-queue",
#             "ROUTING_KEY" : "live.segmentation.cmd.run"
#         }
#     ]
    
# }
