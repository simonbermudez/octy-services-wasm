#module imports


#python imports
from os import execlp
from uuid import uuid4
from datetime import date, datetime
import json
import base64
from typing import Union

#external imports
from basicauth import DecodeError, decode

#external imports
import requests
from requests.adapters import HTTPAdapter
from requests.packages.urllib3.util.retry import Retry


######################################
# global utils functions
######################################

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

def int_to_dt(dt_int : int, as_str : bool) -> object:
    """
        A utility function used to convert a integer datetime to datetime object or formatted string.

        Parameters
        ----------
        dt_int : int

        Returns
        ----------
        datetime or formatted str
    """
    date_obj = datetime.fromtimestamp(dt_int / 1e3)
    if as_str:
      return date_obj.strftime('%a, %d %b %Y %H:%M:%S GMT')

    return date_obj

def str_to_dt(dt_str : str) -> object:
    """
        A utility function used to convert a string datetime to datetime object.

        Parameters
        ----------
        dt_str : str

        Returns
        ----------
        datetime
    """
    datetime_obj = datetime.strptime(dt_str, '%Y-%m-%dT%H:%M:%S.%f')
    return datetime_obj

def generate_uid(prefix : str) -> str:
    """
        A utility function used to generate a UID.

        Parameters
        ----------
        prefix : str
            The prefix that should be appended to the start of the uid

        Returns
        ----------
        uid : str
    """
    # Some uids will be consumed by services that have formatting restrictions.
    uid_formatting = {
        'bucket' : {
          'len' : 27,
          'seperator' : '-'
        },
        'training-job' :{
          'len' : 22,
          'seperator' : '-'
        },
        'notification' : {
          'len' : 20,
          'seperator' : '-'
        }
    }

    try:
        length = uid_formatting[prefix]['len']
        seperator = uid_formatting[prefix]['seperator']
    except KeyError:
        length = 34
        seperator = '_'

    return prefix+seperator+str(uuid4())[:length]



def basic_auth_parse(token: str) -> Union[bool, str, str]:
    """
        A utility function used to decrypt and parse a Basic Authorization token.

        Parameters
        ----------
        token : str
            Basic Authorization token provided within 'Authorization' header of a request

        Returns
        ----------
        result : bool
        username : str
        password : str
    """
    if token == "" or token == None:
        return False, "", ""

    if 'Bearer' in token:
        # remove 'bearer' from header
        username_password = token[7:].split(":")
        # split on ':' character
        username = username_password[0]
        if len(username_password) > 1:
            password = username_password[1]
        else:
            password = ""
        return True, username, password

    username, password = decode(token)
    return True, username, password


def base64_encode_json(json_obj : object) -> str:
  """
      A utility function used to minify and Base64 encode JSON

      Parameters
      ----------
      json_obj : object
          Json object - secrets or configs

      Returns
      ----------
      Base64 string
  """
  return base64.b64encode(json.dumps(json_obj, \
    separators=(',', ":")).encode('ascii')).decode('ascii')


def base64_decode(b64_str : str, is_json=True) -> object:
    """
        A utility function used to decode Base64 string

        Parameters
        ----------
        b64_str : str
            base64 encoded string
        
        is_json : bool
            is the encoded data a dict or json?

        Returns
        ----------
        Decoded Base64 object
    """
    if is_json:
        try:
            val = json.loads(base64.b64decode(b64_str.encode('ascii')).decode('ascii'))
        except Exception:
            print("Not Base64 string.")
            val = json.loads(b64_str)
        return val

    val = base64.b64decode(b64_str.encode('ascii')).decode('ascii')
    return val


class f_:
    
    def __init__(self):
        self.f = None

    @staticmethod
    def open(path, mode):
        obj = f_()
        obj.f = open(path, mode)
        return obj

    def read(self):
        return self.f.read()

    def close(self):
        return self.f.close()

def loadF(path : str) -> object:
    """
        A utility function used to read K8s config 
        or Secret into python dict.

        Parameters
        ----------
        path : str
            Path to file

        Returns
        ----------
        K8s Config or Secret : object
    """
    return json.load(f_.open(path,"r"))

def json_serial(obj):
    """
        JSON serializer for objects not 
        serializable by default json code

        Parameters
        ----------
        obj : Any
            Object to serialize to json

        Returns
        ----------
        K8s Config or Secret : object
    """
    if isinstance(obj, (datetime, date)):
        return obj.isoformat()
    raise TypeError ("Type %s not serializable" % type(obj))