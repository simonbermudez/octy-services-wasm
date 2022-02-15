from fastapi.responses import JSONResponse


### Get Billable units DTO
class GetBillableUnitsDTO():
    def __init__(self, units, total, cursor):
        self.units = units
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : '', 'count' : len(self.units), 'total' : self.total},
                    'units' : self.units
            },
            headers={'cursor' : str(self.cursor+len(self.units))}
        )

### Get Subscription plans DTO
class GetSubscriptionPlansDTO():
    def __init__(self, subscriptions):
        self.subscriptions = subscriptions

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : ''},
                    'subscriptions' : self.subscriptions
            }
        )
