#module imports


#python imports
import json
import base64
from uuid import uuid4

#external imports



######################################
# global utils functions
######################################

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
    return prefix+'_'+str(uuid4())[:34]