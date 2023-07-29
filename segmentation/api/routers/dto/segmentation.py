from fastapi.responses import JSONResponse
import json

### Get Segments DTO
class GetSegmentsDTO():
    def __init__(self, segments, total, cursor):
        self.segments = segments
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Segments found.',
                                 'count' : len(self.segments), 'total' : self.total},
                'segments' : self.segments
            },
            headers={'cursor' : str(self.cursor+len(self.segments))}
        )

### Create Segment DTO
class CreateSegmentDTO():
    def __init__(self, segment, message):
        self.segment = segment
        self.message = message

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': self.message},
                'segment_id' : self.segment['segment_id'],
                'segment_name' : self.segment['segment_name'],
                'segment_type' : self.segment['segment_type'],
                'segment_sub_type' : self.segment['segment_sub_type'],
                'segment_status' : self.segment['segment_status']
            }
        )

### Delete Segments DTO
class DeleteSegmentsDTO():
    def __init__(self, deleted_segments, failed_to_delete):
        self.deleted_segments = deleted_segments
        self.failed_to_delete = failed_to_delete

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                'request_meta': {'request_status': 'Success',
                                 'message': 'Segments flagged to be deleted.'},
                'deleted_segments' : self.deleted_segments,
                'failed_to_delete' : self.failed_to_delete
            }
        )

class DeleteAccountSegmentationsDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Octy Jobs deleted.'},
                    'is_deleted' : self.is_deleted
            }
        )