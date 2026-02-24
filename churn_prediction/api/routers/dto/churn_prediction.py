from fastapi.responses import JSONResponse


### Generate Churn Report DTO
class GenerateChurnReportDTO():
    def __init__(self, churn_prediction_report):
        self.churn_prediction_report = churn_prediction_report

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Successfully generated churn report'},
                    'churn_prediction_report' : self.churn_prediction_report
            }
        )
    

class DeleteAccountChurnPredictionsDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Octy account churn predictions deleted.'},
                    'is_deleted' : self.is_deleted
            }
        )