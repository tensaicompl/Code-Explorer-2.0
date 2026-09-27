"""Long-lived API tokens a user issues for themselves.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from .. import config
from ..schemas import ApiTokenCreateRequest
from ..warden import issue_api_token, list_api_tokens, revoke_api_token
from .guard import require_sso_identity

logger = logging.getLogger("praxevia.routers.access_keys")

router = APIRouter()

@router.get("/api/api-keys")
async def api_keys_list(http_request: Request):
    user_email = require_sso_identity(http_request)
    static = [
        {"name": client_name, "key_prefix": key[:12]}
        for key, client_name in config.API_KEYS.items()
    ]
    return {"keys": list_api_tokens(user_email), "static_keys": static}


@router.post("/api/api-keys")
async def api_keys_create(body: ApiTokenCreateRequest, http_request: Request):
    user_email = require_sso_identity(http_request)
    name = body.name.strip()[:100]
    if not name:
        raise HTTPException(status_code=400, detail="Key name is required")
    return issue_api_token(user_email, name)


@router.delete("/api/api-keys/{key_id}")
async def api_keys_revoke(key_id: str, http_request: Request):
    user_email = require_sso_identity(http_request)
    if not revoke_api_token(key_id, user_email):
        raise HTTPException(status_code=404, detail="API key not found")
    return {"ok": True}
