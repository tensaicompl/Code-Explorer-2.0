"""Per-user UI settings.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, Request

from ..schemas import PreferencesPayload
from ..storage import load_preferences, store_preferences
from .guard import identify_caller

logger = logging.getLogger("praxevia.routers.preferences")

router = APIRouter()

@router.get("/api/preferences")
async def get_user_preferences(http_request: Request):
    user_identity = identify_caller(http_request)
    return {"preferences": load_preferences(user_identity)}


@router.put("/api/preferences")
async def save_user_preferences(body: PreferencesPayload, http_request: Request):
    user_identity = identify_caller(http_request)
    existing = load_preferences(user_identity)
    incoming = body.model_dump(exclude_none=True)
    merged = {**existing, **incoming}
    saved = store_preferences(user_identity, merged)
    return {"preferences": saved}
