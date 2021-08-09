#module imports


#python imports
from uuid import uuid4
from datetime import datetime as dt
from datetime import datetime
from typing import Union
import json
import base64

#external imports



######################################
# global utils functions
######################################
class DictConditional(dict):
    def __init__(self, cond=lambda x: x is not None):
        self.cond = cond
    def __setitem__(self, key, value):
        if key in self or self.cond(value):
            dict.__setitem__(self, key, value)

def assess_resource_limit(limits : str, current_count : int, requested : int) -> Union[bool, dict]:
    """
        A utility function used to determine if resource limit has been reached for this account

        Parameters
        ----------
        limits : str
            limits set in Auth JWT 
            [50000*150*100*100000*25*50](profiles,items,event_types,events,segments,mes_templates)
        current_count : int
            the current number of existing resources
        requested : int
            the number of resources the client is wishes to created

        Returns
        ----------
        result : bool
        counts : dict
    """
    resource_limit = int(limits.split('*')[1]) # change index to obtain required limit
    remainder = resource_limit - current_count
    exceeded_by = requested - remainder

    counts = {
        'limit' : resource_limit,
        'count_before' : current_count,
        'count_after' : current_count,
        'remainder' : remainder,
        'exceeded_by' : exceeded_by
    }

    if requested + current_count > resource_limit:
        return False, counts
    else:
        counts['count_after']=current_count+requested
        counts['remainder']=remainder-requested
        return True, counts

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
    date_obj = dt.fromtimestamp(dt_int / 1e3)
    if as_str:
      return date_obj.strftime('%a, %d %b %Y %H:%M:%S GMT')

    return date_obj

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