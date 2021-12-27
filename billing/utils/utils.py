#module imports


#python imports
from uuid import uuid4
from datetime import datetime
import json
import base64

#external imports
from basicauth import decode



######################################
# global utils functions
######################################

def dt_to_int(dt : datetime) -> int:
    '''
        A utility function used to convert a datetime object to an integer.

        Parameters
        ----------
        dt : datetime

        Returns
        ----------
        dt as int : int
    '''
    return int(dt.strftime('%Y%m%d%H%M%S'))


def generate_uid(prefix : str) -> str:
    '''
        A utility function used to generate a UID.

        Parameters
        ----------
        prefix : str
            The prefix that should be appended to the start of the uid

        Returns
        ----------
        uid : str
    '''
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