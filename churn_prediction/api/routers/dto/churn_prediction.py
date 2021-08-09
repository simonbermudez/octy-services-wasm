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