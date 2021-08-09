#module imports


#python imports
import json
import base64

#external imports



######################################
# global utils functions
######################################

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