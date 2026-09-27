"""What the caller is allowed to explore, and how much of it is indexed.
"""
from __future__ import annotations

import logging
from pathlib import Path

from fastapi import APIRouter, HTTPException, Query, Request

from .. import config
from ..corpus import ensure_corpus_registry
from ..telemetry import list_user_projects
from ..telemetry_report import build_usage_report
from ..warden import list_permitted_projects
from .guard import ensure_project_access, identify_caller, is_full_access_identity

logger = logging.getLogger("praxevia.routers.projects")

router = APIRouter()

@router.get("/api/projects")
async def list_projects(http_request: Request):
    user_identity = identify_caller(http_request)
    privileged = is_full_access_identity(user_identity)

    if privileged or not config.ACCESS_CONTROL_ENABLED:
        permitted = None  # all projects
    else:
        permitted = set(list_permitted_projects(user_identity))

    registry = ensure_corpus_registry()
    projects = []
    for key, info in sorted(registry.items()):
        name = info.get("project", key)
        projects.append({
            "name": name,
            "stream": info.get("stream", ""),
            "registry_key": key,
            "module_count": 0,
            "primary_language": "",
            "index_count": info["chunk_count"],
            "has_codebase": info["codebase_dir"].exists(),
            "has_access": True if permitted is None else (name in permitted),
        })
    return {"projects": projects}


@router.get("/api/clone-report")
async def clone_report(
    http_request: Request,
    project: str = Query(...),
    stream: str = Query(...),
):
    user_identity = identify_caller(http_request)
    ensure_project_access(user_identity, project)
    report_path = Path(config.CODEBASE_DIR) / project / stream / "clone-report.md"
    if not report_path.is_file():
        raise HTTPException(status_code=404, detail="Clone report not found")
    content = report_path.read_text(encoding="utf-8", errors="replace")
    return {"content": content}


@router.get("/api/stats")
async def stats(http_request: Request):
    identify_caller(http_request)
    return build_usage_report(days=30)


@router.get("/api/user-projects")
async def user_projects(http_request: Request):
    identify_caller(http_request)
    return {"user_projects": list_user_projects()}
