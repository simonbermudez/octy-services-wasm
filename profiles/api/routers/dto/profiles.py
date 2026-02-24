from fastapi.responses import JSONResponse


### Get Profiles DTO
class GetProfilesDTO():
    def __init__(self, profiles, total, cursor):
        self.profiles = profiles
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles found.', 'count' : len(self.profiles), 'total' : self.total},
                    'profiles' : self.profiles
            },
            headers={'cursor' : str(self.cursor+len(self.profiles))}
        )

### Create Profiles DTO
class CreateProfilesDTO():
    def __init__(self, created_profiles, failed_to_create,):
        self.created_profiles = created_profiles
        self.failed_to_create = failed_to_create

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles created.', 'count' : len(self.created_profiles)},
                    'profiles' : self.created_profiles,
                    'failed_to_create' : self.failed_to_create
            }
        )

### Update Profiles DTO
class UpdateProfilesDTO():
    def __init__(self, updated_profiles, failed_to_update,):
        self.updated_profiles = updated_profiles
        self.failed_to_update = failed_to_update

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles updated.'},
                    'profiles' : self.updated_profiles,
                    'failed_to_update' : self.failed_to_update
            }
        )

### Delete Profiles DTO
class DeleteProfilesDTO():
    def __init__(self, deleted_profiles, failed_to_delete):
        self.deleted_profiles = deleted_profiles
        self.failed_to_delete = failed_to_delete

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles deleted.'},
                    'deleted_profiles' : self.deleted_profiles,
                    'failed_to_delete' : self.failed_to_delete
            }
        )

### Get Profiles Meta DTO
class GetProfilesMetaDTO():
    def __init__(self, profiles_meta):
        self.profiles_meta = profiles_meta

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Profiles metadata returned.'},
                    'profiles_meta' : self.profiles_meta
            }
        )


### Get Profiles Internal DTO
class GetProfilesInternalDTO():
    def __init__(self, profiles, not_found, total, cursor):
        self.profiles = profiles
        self.not_found = not_found
        self.total = total
        self.cursor = cursor

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=200,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles found.', 'count' : len(self.profiles), 'total' : self.total},
                    'profiles' : self.profiles,
                    'not_found' : self.not_found
            },
            headers={'cursor' : str(self.cursor+len(self.profiles))}
        )
    
### Delete Profiles Internal DTO
class DeleteAccountProfilesDTO():
    def __init__(self, is_deleted):
        self.is_deleted = is_deleted

    def dto(self) -> JSONResponse:
        return JSONResponse(
            status_code=201,
            content={
                    'request_meta' : { 'request_status' : 'Success' , 'message' : 'Customer profiles deleted.'},
                    'is_deleted' : self.is_deleted
            }
        )