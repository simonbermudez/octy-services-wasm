# module imports
from data.repositories.Inotifications_repository import NotificationsInterface
from data.models.db_schemas import tbl_notifications, tbl_accounts
from secrets import Secrets
from config import Config
from utils.utils import *

# python imports
from typing import *
import json
import requests

# external imports
from mailjet_rest import Client
from sentry_sdk import capture_exception


class NotificationsRepository(NotificationsInterface):
    """
        NotificationsRepository
        Handles:
        - Sending emails via Mailjet rest API
        - Sending webhooks

        ...

        Attributes
        ----------
        account : MongoDB Document (optional)
            MongoDB Document instance of tbl_account

    """

    def __init__(self, account: object = None):
            self.account = account

    def email(self, payload: Dict) -> bool:
        """
        A method used to send an email notification

        Parameters
        ----------
        payload : object
            Dictionary object containing message content and meta data


        Returns
        ----------
        result : bool
        """

        # Create Messages array for mailjet api
        notification_id = generate_uid('notification')
        data = {
            'Messages': [
                {
                    "From": {
                        "Email": Config['SUPPORT_EMAIL'],
                        "Name": "Octy.ai"
                    },
                    "To": [
                        {
                            "Email": payload['contact_email_address'],
                            "Name": payload['contact_name']
                        }
                    ],
                    "Subject": payload['subject'],
                    "TextPart": payload['body'],
                    "HTMLPart": "",
                    "CustomID": notification_id
                }
            ]
        }

        # send email
        try:
            mailjet = Client(auth=(Secrets['MAIL_JET_API_KEY'],
                                   Secrets['MAIL_JET_API_SECRET']),
                             version='v3.1')
            result = mailjet.send.create(data=data)
        except Exception as err:
            capture_exception(err)
            return False

        did_succeed = False
        if result.status_code == 200:
            did_succeed = True

        # Create notification reference
        _create_notification_ref(self.account,
                                 payload['body'],
                                 'email',
                                 payload['contact_email_address'],
                                 notification_id,
                                 did_succeed)
        if did_succeed:
            return True
        return False

    def webhook(self, payload: object) -> None:
        """
        A method used to send an webhook notification

        Parameters
        ----------
        payload : object
            Dictionary object containing message content and meta data

        Returns
        ----------
        None
        """

        notification_id = generate_uid('notification')

        # send webhook request
        result = requests.post(self.account.account_configurations.webhook_url,
                               data=json.dumps(payload))
        did_succeed = False
        if 200 < result.status_code < 300:
            did_succeed = True

        # Create notification reference
        _create_notification_ref(self.account,
                                 payload,
                                 'webhook',
                                 self.account.account_configurations.webhook_url,
                                 notification_id,
                                 did_succeed)


# Helpers

def _create_notification_ref(account,
                             notification_content : object,
                             notification_type : str,
                             destination : str,
                             notification_id : str,
                             did_succeed : bool) -> None:
    """
    A helper function used to create a log of notification
    and it's delivery status.

    Parameters
    ----------
    account : MongoDB Document
            MongoDB Document instance of an tbl_account

    notification_content : object
        Dictionary object containing message content and meta data

    notification_type : str
        The type of notification. email or webhook

    destination : str
        the email address or webhook url the notification was sent to.

    notification_id : str
        Octy generated unique identifier

    did_succeed : bool
        Whether the notification sent successfully or failed.


    Returns
    ----------
    None
    """

    try:

        tbl_notifications(
            notification_id=notification_id,
            account=account,
            notification_content=json.dumps(notification_content),
            notification_type=notification_type,
            destination=destination,
            did_succeed=did_succeed
        ).save()

    except Exception as err:
        capture_exception(err)
