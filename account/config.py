from utils.utils import base64_decode
import os
import json

#os.environ.get('CLOUDAMQP_URL', 'amqp://guest:guest@localhost:5672/%2f')

try:
    Config = json.loads(base64_decode(os.environ.get('ACCOUNT_CONFIG')))
except TypeError:
    Config = base64_decode(os.environ.get('ACCOUNT_CONFIG'))

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