#module imports


#python imports
from os import execlp
from uuid import uuid4
from datetime import datetime
import json
import base64
from typing import Union

#external imports
from basicauth import DecodeError, decode



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