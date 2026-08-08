"""Administering streams and the repository sources feeding them.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Request

from ..corpus import refresh_corpus_registry
from ..harvest import add_stream, attach_repository_source, detach_repository_source, edit_repository_source, remove_stream, set_stream_enabled
from ..schemas import RepositorySourceCreateRequest, RepositorySourceUpdateRequest, StreamCreateRequest, StreamUpdateRequest
from .guard import ensure_admin, identify_caller, to_json_safe

logger = logging.getLogger("praxevia.routers.admin_streams")

router = APIRouter()

@router.post("/api/admin/managed-projects/{project_id}/streams")
async def admin_create_stream(project_id: str, body: StreamCreateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    try:
        result = add_stream(project_id, body.stream_name)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if not result:
        raise HTTPException(status_code=409, detail="Stream already exists for this project")
    refresh_corpus_registry()
    return {"ok": True, "stream": to_json_safe(result)}


@router.put("/api/admin/streams/{stream_id}")
async def admin_update_stream(stream_id: str, body: StreamUpdateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not set_stream_enabled(stream_id, enabled=body.enabled):
        raise HTTPException(status_code=404, detail="Stream not found")
    refresh_corpus_registry()
    return {"ok": True}


@router.delete("/api/admin/streams/{stream_id}")
async def admin_delete_stream(stream_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not remove_stream(stream_id):
        raise HTTPException(status_code=404, detail="Stream not found")
    refresh_corpus_registry()
    return {"ok": True}


# ── Admin: Repository Sources ────────────────────────────────────────


@router.post("/api/admin/streams/{stream_id}/sources")
async def admin_add_source(stream_id: str, body: RepositorySourceCreateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    result = attach_repository_source(
        stream_id=stream_id, source_mode=body.source_mode,
        credential_id=body.credential_id,
        hosting_project_key=body.hosting_project_key, slug_filter=body.slug_filter,
        repo_url=body.repo_url, repo_slug=body.repo_slug,
        branch_strategy=body.branch_strategy, fixed_branch=body.fixed_branch,
    )
    if not result:
        raise HTTPException(status_code=400, detail="Failed to add source")
    return {"ok": True, "source": to_json_safe(result)}


@router.put("/api/admin/sources/{source_id}")
async def admin_update_source(source_id: str, body: RepositorySourceUpdateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not edit_repository_source(
        source_id, credential_id=body.credential_id,
        hosting_project_key=body.hosting_project_key, slug_filter=body.slug_filter,
        repo_url=body.repo_url, repo_slug=body.repo_slug,
        branch_strategy=body.branch_strategy, fixed_branch=body.fixed_branch,
    ):
        raise HTTPException(status_code=404, detail="Source not found")
    return {"ok": True}


@router.delete("/api/admin/sources/{source_id}")
async def admin_delete_source(source_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if not detach_repository_source(source_id):
        raise HTTPException(status_code=404, detail="Source not found")
    return {"ok": True}


@router.post("/api/admin/streams/{stream_id}/discover")
async def admin_discover_repos(stream_id: str, http_request: Request):
    """Dry-run: preview repos that would be cloned for a stream's wildcard sources."""
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    from ..harvest import list_repository_sources
    from ..storage import fetch_credential_with_secret
    from ..harvest import resolve_provider

    sources = list_repository_sources(stream_id)
    all_repos = []
    for source in sources:
        if source["source_mode"] != "wildcard":
            continue
        cred_id = source.get("credential_id")
        if not cred_id:
            continue
        cred = fetch_credential_with_secret(cred_id)
        if not cred:
            continue
        provider = resolve_provider(cred["source_type"], cred["base_url"], cred["token"], cred["ssl_verify"])
        active, _archived = provider.discover_repos(
            source.get("hosting_project_key", ""), source.get("slug_filter", "*"),
        )
        for r in active:
            all_repos.append({"project_key": r.project_key, "slug": r.slug})
    return {"repos": all_repos}


# ── Admin: Refresh & Schedule ────────────────────────────────────────
