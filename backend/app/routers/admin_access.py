"""Administering who may see what: requests, grants and the admin roster.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Query, Request

from ..schemas import AdminGrantRequest, PermissionGrantRequest
from ..telemetry import list_user_projects
from ..warden import approve_request, deny_request, grant_admin, grant_project_permission, list_permissions, list_platform_admins, list_requests, revoke_admin, revoke_project_permission
from .guard import ensure_admin, identify_caller

logger = logging.getLogger("praxevia.routers.admin_access")

router = APIRouter()

@router.get("/api/admin/access-requests")
async def admin_list_access_requests(
    http_request: Request,
    status: str = Query(default=None),
):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    return {"requests": list_requests(status)}


@router.post("/api/admin/access-requests/{request_id}/approve")
async def admin_approve_request(request_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not approve_request(request_id, user_identity):
        raise HTTPException(status_code=404, detail="Request not found or not pending")
    return {"ok": True}


@router.post("/api/admin/access-requests/{request_id}/deny")
async def admin_deny_request(request_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not deny_request(request_id, user_identity):
        raise HTTPException(status_code=404, detail="Request not found or not pending")
    return {"ok": True}


@router.get("/api/admin/permissions")
async def admin_list_permissions(
    http_request: Request,
    project: str = Query(default=None),
):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    return {"permissions": list_permissions(project)}


@router.post("/api/admin/permissions")
async def admin_grant_permission(body: PermissionGrantRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    grant_project_permission(body.user_email, body.project, user_identity)
    return {"ok": True}


@router.delete("/api/admin/permissions")
async def admin_revoke_permission(
    http_request: Request,
    user_email: str = Query(...),
    project: str = Query(...),
):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not revoke_project_permission(user_email, project):
        raise HTTPException(status_code=404, detail="Permission not found")
    return {"ok": True}


@router.get("/api/admin/admins")
async def admin_list_admins(http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    return {"admins": list_platform_admins()}


@router.post("/api/admin/admins")
async def admin_add_admin(body: AdminGrantRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    grant_admin(body.user_email, user_identity)
    return {"ok": True}


@router.delete("/api/admin/admins/{email}")
async def admin_remove_admin(email: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if email.lower() == user_identity.lower():
        raise HTTPException(status_code=400, detail="Cannot remove yourself as admin")
    if not revoke_admin(email):
        raise HTTPException(status_code=404, detail="Admin not found")
    return {"ok": True}


@router.post("/api/admin/migrate-permissions")
async def admin_migrate_permissions(http_request: Request):
    """One-time migration: grant all users in user_projects table access to their historically used projects."""
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    user_projects_list = list_user_projects()
    count = 0
    for up in user_projects_list:
        if grant_project_permission(up["user_email"], up["project"], "migration"):
            count += 1
    return {"migrated": count}


# ── Admin: Credentials ───────────────────────────────────────────────
