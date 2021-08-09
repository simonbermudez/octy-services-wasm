# module imports
from data.repositories.Ibucket_repository import BucketInterface
from secrets import Secrets
from config import Config as C

# python imports
import json

# external imports
import boto3
from botocore.client import Config
from sentry_sdk import capture_exception


class BucketRepository(BucketInterface):
    """
        BucketRepository
        Handles:
        - Create a bucket (aws s3)
        - Configure bucket
        - Create a directory within specified bucket

        ...

        Attributes
        ----------
        account : Mongo Document
            Octy account
    """
    def __init__(self, account):
        self.account = account

        # s3 resource object
        self.s3_resource = boto3.resource('s3',
         aws_access_key_id=Secrets['AWS_ACCESS_KEY_ID'],
         aws_secret_access_key=Secrets['AWS_SECRET_ACCESS_KEY'])

        # s3 client object
        self.s3_client = boto3.client('s3',
         aws_access_key_id=Secrets['AWS_ACCESS_KEY_ID'],
         aws_secret_access_key=Secrets['AWS_SECRET_ACCESS_KEY'])


    def create_bucket(self, bucket_name: str) -> bool:
        """
        A method used to create an AWS s3 bucket instance.
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        Returns
        ----------
        result : bool
        """
        try:
            self.s3_client.create_bucket(Bucket=bucket_name,
                                         CreateBucketConfiguration={'LocationConstraint': C['AWS_REGION']})
        except Exception as err:
            capture_exception(err)
            return False
        return True

    def bucket_configuration(self, bucket_name: str) -> bool:
        """
        A method used to configure an AWS s3 bucket to
        conform to Octy requirements

        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        Returns
        ----------
        result :  bool
        """
        try:

            # Define the configuration rules
            cors_configuration = {
                'CORSRules': [{
                    'AllowedHeaders': ['*','Access-Control-Expose-Headers'],
                    'AllowedMethods': ['GET','POST','PUT'],
                    'AllowedOrigins': ['*'],
                    'ExposeHeaders': ['GET', 'PUT', 'ETag'],
                    'MaxAgeSeconds': 3000
                }]
            }
            self.s3_client.put_bucket_cors(Bucket=bucket_name,
                    CORSConfiguration=cors_configuration)

            # Define and apply a bucket policy
            bucket_policy = {
            "Version": "2012-10-17",
            "Id": "S3PolicyIPRestrict",
            "Statement": [
                {
                    "Sid": "IPAllow",
                    "Effect": "Allow",
                    "Principal": {
                        "AWS": "*"
                    },
                    "Action": "s3:*",
                    "Resource": "arn:aws:s3:::"+bucket_name+"/*",
                    "Condition" : {
                        "IpAddress" : {
                            "aws:SourceIp": C['AWS_ALLOWED_IP']
                        },
                        "NotIpAddress" : {
                            "aws:SourceIp": "192.168.143.188/32"
                        }
                    }
                }
                ]
            }
            self.s3_client.put_bucket_policy(Bucket=bucket_name, Policy=json.dumps(bucket_policy))

            # tag AWS resource with Octy account ID, for cost tracking
            bucket_tagging = self.s3_resource.BucketTagging(bucket_name)
            bucket_tagging.put(
                Tagging = {
                    'TagSet' : [{'Key': 'octy_account_id', 'Value': str(self.account.account_id)}]
            })

        except Exception as err:
            capture_exception(err)
            return False
        return True

    def create_directory(self, bucket_name: str, directory_path : str) -> None:
        """
        A method used to create a directory within the specified AWS s3 bucket

        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        directory_path : str
            Path where the new directory should be created

        Returns
        ----------
        result : None
        """
        self.s3_client.put_object(Bucket=bucket_name, Key=(directory_path + '/'))