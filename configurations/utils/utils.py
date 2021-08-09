#module imports


#python imports
from os import execlp
from uuid import uuid4
from datetime import datetime
import requests
from requests.adapters import HTTPAdapter
from requests.packages.urllib3.util.retry import Retry
import json
import base64

#external imports

######################################
# global utils functions
######################################

def dt_to_int(dt : datetime) -> int:
    """
        A utility function used to convert a datetime object to an integer.

        Parameters
        ----------
        dt : datetime

        Returns
        ----------
        dt as int : int
    """
    return int(dt.strftime("%Y%m%d%H%M%S"))


def requests_retry_session(
    retries=4,
    backoff_factor=0.3,
    status_forcelist=(500, 502, 504),
    session=None
):
    """
        A utility function used manage retrying 
        failed [500, 502, 504] HTTP requests.

        Parameters
        ----------
        null

        Returns
        ----------
        session : requests.Session()
    """
    session = session or requests.Session()
    retry = Retry(
        total=retries,
        read=retries,
        connect=retries,
        backoff_factor=backoff_factor,
        status_forcelist=status_forcelist,
    )
    adapter = HTTPAdapter(max_retries=retry)
    session.mount('http://', adapter)
    session.mount('https://', adapter)
    return session

def base64_decode(b64_str : str) -> object:
  """
      A utility function used to decode Base64 string

      Parameters
      ----------
      b64_str : str
          base64 encoded string

      Returns
      ----------
      Decoded Base64 object
  """
  try:
    val = json.loads(base64.b64decode(b64_str.encode('ascii')).decode('ascii'))
  except Exception:
    print("Not Base64 string.")
    val = json.loads(b64_str)

  return val