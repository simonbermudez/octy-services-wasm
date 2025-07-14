# module imports
from data.repositories.Inotifications_repository import NotificationsInterface
from secrets import Secrets
from config import Config
from utils.utils import *
import data.context.db_context as ctx

# python imports
from typing import *
import json
import requests

# external imports
from mailjet_rest import Client
from sentry_sdk import capture_exception
from datetime import datetime


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
    def __init__(self, account: dict = None):
        self.account = account
        self.collection = lambda: ctx.contextManager.db["tbl_notifications"]

    async def email(self, payload: Dict) -> bool:
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

        data_to_octy = {
            'Messages': [
                {
                    "From": {
                        "Email": Config['SUPPORT_EMAIL'],
                        "Name": "Octy.ai"
                    },
                    "To": [
                        {
                            "Email": "ops@octy.ai",
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

        try:
            mailjet = Client(auth=(Secrets['MAIL_JET_API_KEY'], Secrets['MAIL_JET_API_SECRET']), version='v3.1')
            to_client_result = mailjet.send.create(data=data)
            to_octy_result = mailjet.send.create(data=data_to_octy)
        except Exception as err:
            capture_exception(err)
            return False

        did_succeed = to_octy_result.status_code == 200 and to_client_result.status_code == 200

        await self._create_notification_ref(payload['body'], 'email', payload['contact_email_address'], notification_id, did_succeed)
        return did_succeed

    async def webhook(self, payload: object) -> None:
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

        try:
            result = requests.post(self.account['account_configurations']['webhook_url'], data=json.dumps(payload))
            did_succeed = result.status_code >= 200 and result.status_code < 300
        except Exception as e:
            capture_exception(e)
            did_succeed = False

        await self._create_notification_ref(payload, 'webhook', self.account['account_configurations']['webhook_url'], notification_id, did_succeed)

    async def _create_notification_ref(self, notification_content: object, notification_type: str, destination: str, notification_id: str, did_succeed: bool) -> None:
        try:
            await self.collection().insert_one({
                "notification_id": notification_id,
                "account_id": self.account.get("account_id"),
                "notification_content": json.dumps(notification_content),
                "notification_type": notification_type,
                "destination": destination,
                "did_succeed": did_succeed,
                "created_at": datetime.utcnow()
            })
        except Exception as err:
            capture_exception(err)


