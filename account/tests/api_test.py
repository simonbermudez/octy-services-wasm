# module imports
from api import *
from data.context.db_context import contextManager

# python imports
import json
import logging

# external imports
from fastapi.testclient import TestClient
import pytest


client = TestClient(app)

logger = logging.getLogger('test-logger')

######################################
# Account API TESTS:
######################################

# @pytest.mark.asyncio
async def test_get_accounts_internal():
    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)
    response = client.post("/v1/internal/accounts",
        headers={"cursor": "0"},
        json={'account_ids' : [
            'account_8adf8159-5f82-4af1-9b76-9cb71ded17'
        ]}
    )
    print(response.text)
    assert response.status_code == 200

######################################
# Auth API TESTS:
######################################

# @pytest.mark.asyncio
async def test_authenticate_account():
    # Connect to mongoDB
    await contextManager.db_connect(logger=logger)
    response = client.get("/v1/account/authenticate",
        headers={"Authorization": "Basic cGtfMmRlNGJmOTItNmQwNC00ODVhLTlkMDktMjViNGJmMDkxZDpza18xYzBhNjA1Yi1kMWU3LTQ1NjEtYWI4OC05NmU5YWI4MTQ2"}
    )
    assert response.status_code == 200

    # Assert keys in response
    assert "auth" in response.json()
    assert "jwt_token" in response.json()['auth']

    # Assert data type
    assert type(response.json()['auth']['jwt_token']) is str
