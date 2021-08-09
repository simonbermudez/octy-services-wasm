from utils.utils import base64_decode
import os
import json

try:
    Config = json.loads(base64_decode(os.environ.get('CHURN_PREDICTION_WORKER_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('CHURN_PREDICTION_WORKER_CONFIG'))

# Config = {
#     "ENV" : "churn_prediction.worker.development",
#     "SENTRY_URL" : "https://e4b80290888a4267a2224efc6dbed258@o324132.ingest.sentry.io/1826169",
#     "DB_ALIAS" : "churn_prediction_db",
#     "EVENT_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "PROFILE_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "ITEM_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "SEGMENTATION_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "OCTY_JOB_SERVICE_CLUSTER_IP" : "https://api.octy.ai",
#     "DATA_SET_TIMEFRAME" : 1051200,
#     "MIN_NUM_PROFILES" : 300,
#     "MIN_NUM_ROWS_COLLECTIVE" : 400,
#     "MIN_NUM_ITEMS" : 10,
#     "ALLOWED_COL_NULL_COUNT" : 35,
#     "MIN_NUM_UNIQUE_NUMERICAL_COL_VALUES" : 2,
#     "EVENTS_DATAFRAME_COLS" : ["profile_id", "variable_value"],
#     "ITEMS_DATAFRAME_COLS" : ["item_id", "item_categories", "item_name","item_description", "item_price"],
#     "ITEM_FEATURE_COLS" : ["item_categories", "item_name", "item_description", "item_price"],
#     "PROFILES_DIR" : "resources/raw_data/profiles",
#     "ITEMS_DIR" : "resources/raw_data/items",
#     "CHURN_DATA_DIR" : "resources/training_job_data/churn_prediction_data",
#     "MAX_CHUNK_SIZE" : 100000000,
#     "MIN_CHUNK_SIZE" : 6000000,
#     "MIN_FILE_SIZE" : 15000000,
#     "MAX_FILE_SIZE" : 50000000000,
#     "MIN_FILE_SIZE_SINGLE" : 1000000,
#     "MAX_NUM_PARTS" : 10000,
#     "CHURN_PRED_MODELS_DIR" : "models/churn_prediction_models",
#     "CHURN_ALGORITHM_DOCKER_PATH" : "456239226913.dkr.ecr.eu-west-2.amazonaws.com/octy/churn_prediction:latest",
#     "CHURN_SM_INPUT_MODE" : "File",
#     "CHURN_TRAINING_HYPERPARAMETERS" : {"learning_rate" : "0.1", "n_estimators" : "140", "max_depth" : "5", "min_child_weight" :"1", "gamma":"0", "subsample":"0.8", "colsample_bytree":"0.8", "objective": "binary:logistic",  "nthread":"4", "scale_pos_weight":"1", "seed":"27"},
#     "AWS_REGION" : "eu-west-2",
#     "AWS_SERVER_SIDE_ENCRYPTION" : "AES256",
#     "AWS_ROLE_ARN" : "arn:aws:iam::456239226913:role/AmazonSageMaker",
#     "EC2_INSTANCE_TYPE" : "ml.m5.large",
#     "VOLUME_SIZE_GB" : 50,
#     "TRAINING_MAX_RUN_TIME" : 3600,
    
#     "AMQP_URL" : "amqps://kcditigk:jckigtvlFqNxH652hdYIqRPQH7kPuS0n@brilliant-grey-impala.rmq2.cloudamqp.com/kcditigk",
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
#             "QUEUE" : "profiles-update-queue",
#             "ROUTING_KEY" : "profiles.cmd.update"
#         },
#         {
#             "QUEUE" : "churn-info-update-queue",
#             "ROUTING_KEY" : "churn.info.cmd.update"
#         }
#     ],
#     "AMQP_CONSUMERS" : [

#         {
#             "QUEUE" : "churn-training-run-queue",
#             "ROUTING_KEY" : "churn.training.cmd.run"
#         },
#         {
#             "QUEUE" : "churn-training-complete-queue",
#             "ROUTING_KEY" : "churn.training.complete.cmd.run"
#         }
#     ]
    
# }
