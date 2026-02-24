# module imports
from data.repositories.implementation.auth_repository import authRepository
from data.repositories.implementation.account_repository import accountRepository
from data.repositories.implementation.notifications_repository import NotificationsRepository
from data.repositories.content.notification_content import AUTH_SECURITY_WARNING_SUBJECT, AUTH_SECURITY_WARNING_BODY
from api.routers.error_handlers import *
from config import Config
from utils.utils import *

# python imports
import re
from datetime import datetime as dt

# external imports
from fastapi import Request, HTTPException

class AuthService:
    """
        AuthService 
        Handles:
        - Account authentication
        - Get Auth token

        ...

        Attributes
        ----------
        none
    """
    def __init__(self): pass


    async def validate_auth_request_headers(self, request : Request) -> None:
        """
            A method used to verify required auth headers have been provided.

            Parameters
            ----------
            request : Starlette Request instance
                Basic Authorization token provided within 'Authorization' header of a request

            Returns
            ----------
            None
        """
        try:
            token =  request.headers['authorization']
        except KeyError:
            raise OctyException(400,'Missing header',[{'error_message' : '[Authorization] : [Basic ...] header must be provided in request headers.', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])
        
        res,pk,sk = basic_auth_parse(token)
        if res == False:
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Please provide public and secret keys, encoded as a basic authorization token, within the \'Authorization\' header of this request.', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

        if pk == "" or pk == None:
            _log_failed_auth(request, False)
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Please provide your Octy public key (username), encoded as a basic authorization token, within the Authorization header of this request.', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

        if sk == "" or sk == None:
            _log_failed_auth(request, False)
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Please provide your Octy secret key (password), encoded as a basic authorization token, within the Authorization header of this request.', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

        # Assert the formats of each supplied key to ensure we have one pk and one sk
        if not re.match(r'[p][k][_][a-zA-Z0-9]',pk):
            _log_failed_auth(request, False)
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Invalid public_key or secret_key provided', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

        if not re.match(r'[s][k][_][a-zA-Z0-9]',sk):
            _log_failed_auth(request, False)
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Invalid public_key or secret_key provided', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

    async def authenticatation(self, request : Request) -> str:
        """
            A method used to validate provided crednetials 
            and return an authorization JWT

            Parameters
            ----------
            request : Starlette Request instance
                Basic Authorization token provided within 'Authorization' header of a request

            Returns
            ----------
            Auth JWT : str
                Account Auth (fat jwt) containing account info + authorized resource tags
        """
        _,pk,sk = basic_auth_parse(request.headers['authorization'])
        # update account cache
        await accountRepository.refresh_account_data_cache(pk)
        valid_pk, valid_sk, account = await authRepository.verify_account_keys(pk, sk)
        if not valid_pk or not valid_sk:
            _log_failed_auth(request, valid_pk)
            raise OctyException(401,'Authentication failed', [{'error_message' : 'Invalid public_key or secret_key provided', 
                'extended_help': Config['AUTH_EXTENDED_HELP']}])

        # assess api request limits
        #limit_not_exceeded = await _assess_request_limit(account)
        #if not limit_not_exceeded:
        #    raise OctyException(400,'limit exceeded', [{'error_message' : 'You have exceeded this accounts monthly API request limit. To increase this limit, please contact us at support@octy.ai', 
        #        'extended_help': Config['RATE_LIMIT_EXTENDED_HELP']}])

        # update account cache. increment request count

        return await authRepository.generate_authorization_token(account=account)


# Helpers

async def _assess_request_limit(account : dict) -> bool:
    """
    A function used to determine if this request should be processed in accordance with this accounts
    api request limits.
    """
    try:
        created_at = int_to_dt(account['created_at']['$date'], as_str=False)
    except:
        created_at = str_to_dt(account['created_at'])
    now = dt.now()
    delta = now - created_at
    month_number = round(delta.days / 30)
    month = next((u for u in account['api_usage'] if u['month'] == month_number), None)
    if month != None:
        if month['request_count']+1 >= Config['MAX_TOTAL_MONTHLY_API_REQUESTS'][account['account_configurations']['account_type']]:
            return False
        for acc in account['api_usage']:
            if acc['month'] == month['month']:
                acc['request_count'] += 1
                break
        await accountRepository.update_account_cache(account)
    else:
        account['api_usage'].append({
            'month' : month_number,
            'request_count' : 1
        })
        await accountRepository.update_account_cache(account)
    return True

def _log_failed_auth(request : Request, valid_pk : bool) -> None:
    """
        A function used to log failed authentication requests.
        Alert account holder of 20 number of failed auth attempts, from any single public key.

        Parameters
        ----------
        request : Starlette Request instance

        invalid_pk : bool
            Was the private key invalid? If so, we do not need to log it.

        Returns
        ----------
        None
    """

    if not valid_pk:
        return

    #desealize request object
    try:
        user_agent=request.headers['user-agent']
    except KeyError:
        user_agent='Not supplied'
    server_name=request.client.host
    server_port=request.client.port
    request_type=request.method

    res, pk, _ = basic_auth_parse(request.headers['authorization'])
    if res == False:
        return

    failed_attempt = {
        'public_key' : pk,
        'user_agent' : user_agent,
        'server_name' : server_name,
        'server_port' : server_port,
        'request_type' : request_type
    }
    auth_attempts = authRepository.log_failed_auth(failed_attempt)

    if len(auth_attempts) > Config['FAILED_AUTH_ATTEMPT_LIMIT']:

        account = accountRepository.get_account(pk=pk, dict=False)
        # send email notification
        NotificationsRepository(account=account)\
            .email(
                {
                    'contact_email_address' : account.account_configurations.contact_email_address,
                    'contact_name' : account.account_configurations.contact_name,
                    'subject' : AUTH_SECURITY_WARNING_SUBJECT,
                    'body' : AUTH_SECURITY_WARNING_BODY
                }
        )

#public initalized instance of AuthService object
authService = AuthService()
