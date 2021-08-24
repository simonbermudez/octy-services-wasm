# module imports
from data.repositories.Ibucket_repository import BucketInterface
from secrets import Secrets
from config import Config
from mongoengine.errors import DoesNotExist
from utils.utils import *

# python imports
from datetime import datetime as dt
import json
import io
from typing import Union
import tarfile

# external imports
import boto3
from botocore.client import Config as BConfig
from boto3.s3.transfer import TransferConfig
import botocore
from botocore.exceptions import ClientError


class _BucketRepository(BucketInterface):
    """
        _BucketRepository
        Handles:
        - Mutlipart uploads (aws s3)
        - Single resource uploads
        - Download resource
        - Delete resource
        - Delete directory

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): 
        # Initialize s3 resource object
        self.s3_resource = boto3.resource('s3',
         region_name=Config['AWS_REGION'],
         aws_access_key_id=Secrets['AWS_ACCESS_KEY_ID'],
         aws_secret_access_key= Secrets['AWS_SECRET_ACCESS_KEY'])

        # Initialize s3 client object
        self.s3_client = boto3.client('s3',
         region_name=Config['AWS_REGION'],
         aws_access_key_id=Secrets['AWS_ACCESS_KEY_ID'],
         aws_secret_access_key= Secrets['AWS_SECRET_ACCESS_KEY'])

    async def multipart_upload(self,
                         chunk_data : object,
                         chunk_index : int,
                         resource_friendly_name : str,
                         hyperparam_tuning_job_id : str,
                         bucket_name : str):
        """
        Parameters
        ----------
        chunk_data : object
            Chunk of data being uploaded to bucket

        file_size : int
            Total size (in bytes) of all aggregated chunks. Total file size.

        chunk_index : int
            Current chunk being uploaded.

        chunk_count : int
            The total number of chunks the file has been divided into.

        resource_friendly_name : str
            Unique name of the file being uploaded.

        hyperparam_tuning_job_id : str
            Unique identifier of the relevant hyper paramter tuning job this resource is being uploaded for.
        
        bucket_name : str
            name of aws s3 bucket instance

        Returns
        ----------
        value of upload_part method
        """
        mpu_key = await _generate_file_key(resource_friendly_name, hyperparam_tuning_job_id)
        mpu = self.s3_client.create_multipart_upload(Bucket=bucket_name, Key=mpu_key, ServerSideEncryption=Config['AWS_SERVER_SIDE_ENCRYPTION'])
        parts = []
        return await self.upload_part(chunk_data=chunk_data,
                                    chunk_index=chunk_index,
                                    mpu_key=mpu_key,
                                    upload_id=mpu['UploadId'],
                                    bucket_name=bucket_name,
                                    parts=parts)

    async def upload_part(self,
                    chunk_data : object,
                    chunk_index : int,
                    mpu_key : str,
                    upload_id : str,
                    bucket_name : str,
                    parts : list) -> Union[str, str, list]:
        """
        Parameters
        ----------
        chunk_data : object
            Chunk of data being uploaded to bucket

        chunk_index : int
            Current chunk being uploaded.
        
        mpu_key : str
            Multipart upload key

        upload_id : str
            Third party generated MPU ID.
        
        bucket_name : str
            name of aws s3 bucket instance
        
        parts : list
            List of part ETags

        Returns
        ----------
        mpu_key : str
        upload_id : str
        parts : list
        """
        part = self.s3_client.upload_part(Body=chunk_data,
                                    Bucket=bucket_name,
                                    Key=mpu_key,
                                    PartNumber=chunk_index,
                                    UploadId=upload_id)
        parts.append({'ETag':part['ETag'], 'PartNumber': chunk_index})

        return mpu_key, upload_id, parts
        
    async def complete_multipart_upload(self,
                    mpu_key : str,
                    upload_id : str,
                    bucket_name : str,
                    parts : list) -> None:
        """
        Parameters
        ----------
        mpu_key : str
            Multipart upload key

        upload_id : str
            Third party generated MPU ID.

        bucket_name : str
            name of aws s3 bucket instance

        parts : list
            List of part ETags

        Returns
        ----------
        None
        """
        self.s3_client.complete_multipart_upload(
            Bucket=bucket_name,
            Key=mpu_key,
            MultipartUpload={'Parts': parts},
            UploadId=upload_id
        )
        
    async def abort_multipart_upload(self,
                    key : str,
                    upload_id : str,
                    bucket_name : str) -> None:
        """
        Parameters
        ----------
        key : int
            Complete file path of object in bucket

        upload_id : str
            Third party generated MPU ID.

        bucket_name : str
            The type of resource being created.

        Returns
        ----------
        None
        """
        try:
            self.s3_client.abort_multipart_upload(
                Bucket=bucket_name,
                Key=key,
                UploadId=upload_id
            )
        except: pass
    
    async def single_upload(self,
                         file_data : object,
                         resource_friendly_name : str,
                         hyperparam_tuning_job_id : str,
                         bucket_name : str) -> str:
        """
        Parameters
        ----------
        file_data : object
            Data being uploaded to bucket

        resource_friendly_name : str
            Unique name of the file being uploaded.

        hyperparam_tuning_job_id : str
            Unique identifier of the relevant hyper paramter tuning job this resource is being uploaded for. (optional)

        bucket_name : str
            name of aws s3 bucket instance

        Returns
        ----------
        key : str
        """
        key = await _generate_file_key(resource_friendly_name, hyperparam_tuning_job_id)
        fo = io.BytesIO(file_data.encode())
        self.s3_client.upload_fileobj(Fileobj=fo,Bucket=bucket_name, Key=key)
        return key
        
    async def download_resource(self,
                                bucket_name : str,
                                key : str,
                                is_compressed : bool = False) -> list:
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name
        key : str
            Complete file path of object in bucket
        is_compressed : bool
            Are the files being downloaded compressed?

        Returns
        ----------
        files : list
        """
        files =[]
        s3_obj = self.s3_resource.Object(bucket_name=bucket_name, key=key)
        file_bytes = s3_obj.get()["Body"].read()
        bytes_len = len(file_bytes)
        file_object = io.BytesIO(file_bytes)

        if is_compressed:
            try:
                tarf = tarfile.open(fileobj=file_object, mode="r:gz", bufsize=bytes_len)
            except:
                raise Exception('Error occurred when downloading and decompressing file -- file too small.')

            members = tarf.getmembers()
            for member in members:
                f = tarf.extractfile(member)
                files.append(
                    {
                        'file_name' : member.name,
                        'file_data' : f
                    }
                )
        else:
            files.append(file_object)
        return files
        
    async def delete_directory(self,
                    bucket_name : str,
                    directory_path : str) -> None:
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        directory_path : str
            Complete directory path in bucket. 
            NOTE: All file inside specified directory will be deleted!

        Returns
        ----------
        None
        """
        bucket = self.s3_resource.Bucket(bucket_name)
        for obj in bucket.objects.filter(Prefix=directory_path):
            self.s3_resource.Object(bucket.name,obj.key).delete()


async def _generate_file_key(resource_friendly_name : str, hyperparam_tuning_job_id : str = None) -> str:

    # Generate file name (mpu key)
    key = generate_uid('key')
    if 'meta_data' in resource_friendly_name:
        return Config['REC_DATA_DIR'] + '/'+hyperparam_tuning_job_id+'/' + key+ '.json'
    return Config['REC_DATA_DIR'] + '/'+hyperparam_tuning_job_id+'/' + key+ '.csv'

bucketRepository = _BucketRepository()