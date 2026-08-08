"""Administering managed projects.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from ..corpus import refresh_corpus_registry
from ..harvest import add_project, edit_project, import_filesystem_projects, list_projects_with_detail, remove_project
from ..schemas import ProjectCreateRequest, ProjectUpdateRequest
from .guard import ensure_admin, identify_caller, to_json_safe

logger = logging.getLogger("praxevia.routers.admin_projects")

router = APIRouter()

@router.get("/api/admin/managed-projects")
async def admin_list_managed_projects(http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    projects = list_projects_with_detail()
    return {"projects": [to_json_safe(p) for p in projects]}


@router.post("/api/admin/managed-projects")
async def admin_create_project(body: ProjectCreateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    result = add_project(
        name=body.name, display_name=body.display_name,
        mode=body.mode, created_by=user_identity,
    )
    if not result:
        raise HTTPException(status_code=409, detail="Project name already exists")
    return {"ok": True, "project": to_json_safe(result)}


@router.put("/api/admin/managed-projects/{project_id}")
async def admin_update_project(project_id: str, body: ProjectUpdateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not edit_project(project_id, display_name=body.display_name, mode=body.mode):
        raise HTTPException(status_code=404, detail="Project not found")
    return {"ok": True}


@router.delete("/api/admin/managed-projects/{project_id}")
async def admin_delete_project(project_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not remove_project(project_id):
        raise HTTPException(status_code=404, detail="Project not found")
    refresh_corpus_registry()
    return {"ok": True}


@router.post("/api/admin/managed-projects/migrate-filesystem")
async def admin_migrate_filesystem(http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    count = import_filesystem_projects(user_identity)
    if count > 0:
        refresh_corpus_registry()
    return {"migrated": count}


# ── Admin: Streams ───────────────────────────────────────────────────
