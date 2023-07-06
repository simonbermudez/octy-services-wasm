from fastapi.responses import JSONResponse


### Create Account DTO
class CreateAccountDTO():
    def __init__(self, account_name, account_type, account_currency, contact_email_address, pk, notification_sent):
        self.account_name = account_name
        self.account_type = account_type
        self.account_currency = account_currency
        self.contact_email_address = contact_email_address
        self.pk = pk
        self.notification_sent = notification_sent

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Account created!'},
                    'account_name' : self.account_name,
                    'account_type' : self.account_type,
                    'account_currency' : self.account_currency,
                    'pk' : self.pk,
                    'notification_sent' : self.notification_sent,
                    'sent_to' : self.contact_email_address
            }
        )


### Update Account DTO
class UpdateAccountDTO():
    def __init__(self, contact_email_address, contact_name, contact_surname, webhook_url):
        self.contact_email_address = contact_email_address
        self.contact_name = contact_name
        self.contact_surname = contact_surname
        self.webhook_url = webhook_url

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Account updated!'},
                    'contact_email_address' : self.contact_email_address,
                    'contact_name' : self.contact_name,
                    'contact_surname' : self.contact_surname,
                    'webhook_url' : self.webhook_url
            }
        )


### Get Accounts Internal DTO
class GetAccountsInternalDTO():
    def __init__(self, accounts, total):
        self.accounts = accounts
        self.total = total

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Found Accounts', 'count' : len(self.accounts), 'total' : self.total},
                    'accounts' : self.accounts
            }
        )

## Delete Account DTO
class DeleteAccountDTO():
    def __init__(self, account_id):
        self.account_id = account_id

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': f"Successfully deleted account {self.account_id} and all associated data!"}
            }
        )
