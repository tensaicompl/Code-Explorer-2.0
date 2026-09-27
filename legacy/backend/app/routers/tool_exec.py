"""Running a single search tool without the agent loop.

This is what the MCP surface posts to. The tool name is checked against the
schema-derived allow-list before dispatch.
"""
from __future__ import annotations

import asyncio
import json
import logging
import re

from fastapi import APIRouter, HTTPException, Request

from .. import config
from ..corpus import TOOL_NAMES, ensure_corpus_registry
from ..schemas import ToolInvocationRequest
from ..telemetry import record_event
from ..warden import list_permitted_projects, looks_like_api_token, resolve_api_token_with_name
from .guard import ensure_project_access, identify_caller, is_full_access_identity

logger = logging.getLogger("praxevia.routers.tool_exec")

#: Tool names this endpoint will dispatch.
#:
#: Taken from the schema the model is shown rather than restated. The check
#: below runs before dispatch, so a stale copy here would reject exactly the
#: tools the agent had just been told about -- with no test able to notice,
#: since the suite runs without a model.
EXPOSED_TOOL_NAMES = frozenset(TOOL_NAMES)

router = APIRouter()

@router.post("/api/tools")
async def execute_tool_endpoint(body: ToolInvocationRequest, http_request: Request):
    user_identity = identify_caller(http_request)

    if body.tool_name not in EXPOSED_TOOL_NAMES:
        raise HTTPException(
            status_code=400,
            detail=f"Unknown tool: {body.tool_name!r}. Allowed: {sorted(EXPOSED_TOOL_NAMES)}",
        )

    from ..corpus import run_tool, run_tool_cross_project, resolve_cross_project_scope

    # Build stats identity — distinguish user-generated API keys
    auth_token = http_request.headers.get("Authorization", "")[7:]
    stats_identity = user_identity
    if looks_like_api_token(auth_token):
        key_info = resolve_api_token_with_name(auth_token)
        if key_info:
            stats_identity = f"__userkey__:{key_info['name']}:{key_info['user_email']}"

    if body.cross_project:
        if is_full_access_identity(user_identity) or not config.ACCESS_CONTROL_ENABLED:
            all_projects = list({info["project"] for info in ensure_corpus_registry().values()})
        else:
            all_projects = list_permitted_projects(user_identity)
        permitted_keys = resolve_cross_project_scope(all_projects)
        if not permitted_keys:
            raise HTTPException(status_code=400, detail="No accessible indexed projects")

        record_event(
            event_type="tool_call",
            project="__all__",
            stream="",
            user_identity=stats_identity,
        )

        result_json: str = await asyncio.to_thread(
            run_tool_cross_project, permitted_keys, body.tool_name, body.tool_input
        )
    else:
        ensure_project_access(user_identity, body.project)
        registry_key = (
            re.sub(r'[^a-z0-9]', '_', body.project.lower())
            + "_"
            + re.sub(r'[^a-z0-9]', '_', body.stream.lower())
        )

        if registry_key not in ensure_corpus_registry():
            raise HTTPException(
                status_code=404,
                detail=f"Project '{body.project}/{body.stream}' not found in registry.",
            )

        record_event(
            event_type="tool_call",
            project=body.project or "",
            stream=body.stream or "",
            user_identity=stats_identity,
        )

        result_json = await asyncio.to_thread(
            run_tool, registry_key, body.tool_name, body.tool_input
        )

    return json.loads(result_json)


# ── Chat History Endpoints ──────────────────────────────────────────
