# python imports
from abc import ABC, abstractmethod

class BucketInterface(ABC):

    @abstractmethod
    def multipart_upload(self,
                         chunk_data : object,
                         chunk_index : int,
                         resource_friendly_name : str,
                         training_job_id : str,
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

        training_job_id : str
            Unique identifier of the relevant training job this resource is being uploaded for.
        
        bucket_name : str
            name of aws s3 bucket instance

        Returns
        ----------
        value of upload_part method
        """
        raise NotImplementedError

    @abstractmethod
    def upload_part(self,
                    chunk_data : object,
                    chunk_index : int,
                    mpu_key : str,
                    upload_id : str,
                    bucket_name : str,
                    parts : list):
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
        :rtype: : str
        :rtype: : str
        :rtype: : list
        """
        raise NotImplementedError


    @abstractmethod
    def complete_multipart_upload(self,
                    mpu_key : str,
                    upload_id : str,
                    bucket_name : str,
                    parts : list):
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
        raise NotImplementedError


    @abstractmethod
    def abort_multipart_upload(self,
                    key : str,
                    upload_id : str,
                    bucket_name : str):
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
        raise NotImplementedError


    @abstractmethod
    def single_upload(self,
                         file_data : object,
                         resource_friendly_name : str,
                         training_job_id : str,
                         bucket_name : str):
        """
        Parameters
        ----------
        file_data : object
            Data being uploaded to bucket

        resource_friendly_name : str
            Unique name of the file being uploaded.

        training_job_id : str
            Unique identifier of the relevant training job this resource is being uploaded for. (optional)

        bucket_name : str
            name of aws s3 bucket instance

        Returns
        ----------
        :rtype: str
        """
        raise NotImplementedError


    @abstractmethod
    def download_resource(self,
                        bucket_name : str,
                        key : str,
                        is_compressed : bool = False):
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
        :rtype: list
        """
        raise NotImplementedError


    @abstractmethod
    def delete_directory(self,
                    bucket_name : str,
                    directory_path : str):
        """
        Parameters
        ----------
        bucket_name : str
            Unique bucket name

        directory_path : str
            Complete directory path in bucket. 

        Returns
        ----------
        None
        """
        raise NotImplementedError