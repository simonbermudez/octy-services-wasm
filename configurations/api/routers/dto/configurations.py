from fastapi.responses import JSONResponse
import json

### Account Configs DTO
class AccountConfigsDTO():
    def __init__(self, account):
        self.account = account

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Successfully got account configurations.'},
                'account_data' : {
                    'contact_name' : self.account.account_configurations['c_n'],
                    'contact_surname' : self.account.account_configurations['c_s'],
                    'contact_email_address' : self.account.account_configurations['c_e'],
                    'webhook_url': self.account.account_configurations['we']
                }
            }
        )


### Set Account Configs DTO
class SetAccountConfigsDTO():
    def __init__(self, configs):
        self.configs = configs

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=202,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Accepted. Updating account configurations.'},
                'account_data' : {
                    'contact_name' : self.configs.contact_name,
                    'contact_surname' : self.configs.contact_surname,
                    'contact_email_address' : self.configs.contact_email_address,
                    'webhook_url': self.configs.webhook_url
                }
            }
        )


### Algorithm Configs DTO
class AlgorithmConfigsDTO():
    def __init__(self, account):
        self.account = account

    def dto(self) -> JSONResponse:

        configurations = []
        for c in self.account.algorithm_configurations:
            #pop un-need data
            try:
                c['config_json'].pop('event_type')
            except KeyError:
                pass
            if c['algorithm_name'] == 'rec':
                try:
                    c['config_json'].pop('rec_item_identifier')
                except KeyError:
                    pass
            elif c['algorithm_name'] == 'churn':
                try:
                    c['config_json'].pop('churn_item_identifier')
                except KeyError:
                    pass

            configurations.append(
                {
                    'algorithm_name': c['algorithm_name'],
                    'configurations': c['config_json']
                }
            )
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Current algorithm configurations'},
                'configurations' : configurations
            }
        )

### Set Algorithm Configs DTO
class SetAlgorithmConfigsDTO():
    def __init__(self, algorithm_name, configs):
        self.algorithm_name = algorithm_name
        self.configs = configs

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=202,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Accepted. Setting algorithm configurations.'},
                'configurations' : [{

                    'algorithm_name' : self.algorithm_name,
                    'configurations' : [self.configs]
                }]
            }
        )
