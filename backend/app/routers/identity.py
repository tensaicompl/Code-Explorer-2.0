"""Who the caller is, and how they ask for access they do not yet have.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from ..schemas import AccessRequestSubmission
from ..warden import is_platform_admin, list_permitted_projects, list_requests_for_user, submit_access_request
from .guard import identify_caller, is_full_access_identity

logger = logging.getLogger("praxevia.routers.identity")

router = APIRouter()

@router.get("/api/me")
async def get_current_user(http_request: Request):
    """Return user identity, permissions, admin status."""
    user_identity = identify_caller(http_request)
    privileged = is_full_access_identity(user_identity)
    if privileged:
        return {
            "identity": user_identity,
            "is_platform_admin": True,
            "is_privileged": True,
            "permitted_projects": [],
            "pending_requests": [],
        }
    return {
        "identity": user_identity,
        "is_platform_admin": is_platform_admin(user_identity),
        "is_privileged": False,
        "permitted_projects": list_permitted_projects(user_identity),
        "pending_requests": list_requests_for_user(user_identity),
    }


@router.post("/api/access-requests")
async def request_access(body: AccessRequestSubmission, http_request: Request):
    user_identity = identify_caller(http_request)
    if is_full_access_identity(user_identity):
        raise HTTPException(status_code=400, detail="Privileged users already have full access")
    return submit_access_request(user_identity, body.project)


@router.get("/api/access-requests")
async def list_my_access_requests(http_request: Request):
    user_identity = identify_caller(http_request)
    return {"requests": list_requests_for_user(user_identity)}


# ── Admin Endpoints ───────────────────────────────────────────────────
