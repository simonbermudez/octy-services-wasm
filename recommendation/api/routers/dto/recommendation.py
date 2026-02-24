from fastapi.responses import JSONResponse


### Get Recommendations DTO
class GetRecommendationsDTO():
    def __init__(self, recommendations, training_job_meta):
        self.recommendations = recommendations
        self.training_job_meta = training_job_meta
        self.training_job_id = self.training_job_meta['training_job_id']
        self.model_accuracy_score = self.training_job_meta['auc_score']
        self.model_created_at = self.training_job_meta['model_created_at']

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Successfully predicted recommendations'},
                    'recommendations' : self.recommendations,
                    'model_meta_data' : { 
                        'training_job_id' : self.training_job_id,
                        'model_accuracy_score' : self.model_accuracy_score,
                        'recommender_event_type' : 'charged',
                        'model_created_at' : self.model_created_at
                    }
            }
        )
    
class DeleteAccountRecommendationsDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'All Account Recommendations deleted.'},
                    'is_deleted' : self.is_deleted
            }
        )