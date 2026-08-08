"""The agent endpoint.

Streams Server-Sent Events for the lifetime of an exploration, which may run for
minutes across many tool calls. Enrichment from linked Jira and Confluence
material happens before the model is invoked, so the model sees the referenced
content rather than the bare URL.
"""
from __future__ import annotations

import hashlib
import json
import logging
import traceback

from fastapi import APIRouter, HTTPException, Request
from fastapi.responses import StreamingResponse

from .. import config
from ..cartographer import run_exploration
from ..corpus import ensure_corpus_registry
from ..schemas import ChatPromptRequest
from ..telemetry import record_event, track_user_project
from ..warden import list_permitted_projects, looks_like_api_token, resolve_api_token_with_name
from .guard import ensure_project_access, identify_caller, is_full_access_identity

logger = logging.getLogger("praxevia.routers.chat")

router = APIRouter()

@router.post("/api/chat")
async def chat(request: ChatPromptRequest, http_request: Request):
    logger.info("Chat request: message=%r (len=%d), history=%d msgs, project=%s, cross_project=%s",
                request.message[:80] if request.message else request.message,
                len(request.message), len(request.history), request.project, request.cross_project)
    user_email = identify_caller(http_request)

    # Resolve cross-project keys or single-project registry key
    from ..corpus import sanitize_registry_key, resolve_cross_project_scope
    permitted_keys = None
    registry_key = ""

    if request.cross_project:
        if is_full_access_identity(user_email) or not config.ACCESS_CONTROL_ENABLED:
            all_projects = list({info["project"] for info in ensure_corpus_registry().values()})
        else:
            all_projects = list_permitted_projects(user_email)
        permitted_keys = resolve_cross_project_scope(all_projects)
        if not permitted_keys:
            raise HTTPException(status_code=400, detail="No accessible indexed projects for cross-project search")
    else:
        ensure_project_access(user_email, request.project)
        if request.stream:
            registry_key = f"{sanitize_registry_key(request.project)}_{sanitize_registry_key(request.stream)}"
        else:
            registry_key = request.project

    # Track user-project association
    if user_email and request.project and not request.cross_project:
        track_user_project(user_email, request.project)

    # Record usage stat — derive session_id from auth token to track unique users
    auth_token = http_request.headers.get("Authorization", "")[7:]
    session_hash = hashlib.sha256(auth_token.encode()).hexdigest()[:16]

    # Build stats identity — distinguish user-generated API keys from regular logins
    stats_identity = user_email
    if looks_like_api_token(auth_token):
        key_info = resolve_api_token_with_name(auth_token)
        if key_info:
            stats_identity = f"__userkey__:{key_info['name']}:{key_info['user_email']}"

    record_event(
        event_type="chat_message",
        project="__all__" if request.cross_project else (request.project or ""),
        stream="" if request.cross_project else (request.stream or ""),
        model=request.model or "",
        session_id=session_hash,
        user_identity=stats_identity,
    )

    # Enrich message with Jira issue details if links are present
    from ..integrations import enrich_with_jira_context
    enriched_message = enrich_with_jira_context(request.message, request.jira_token or None)

    # Enrich message with Confluence page content if links are present
    from ..integrations import enrich_with_confluence_context
    conf_token = request.confluence_token or config.CONFLUENCE_TOKEN or None
    enriched_message = enrich_with_confluence_context(enriched_message, conf_token)

    async def event_stream():
        try:
            async for event in run_exploration(
                enriched_message,
                [m.model_dump() for m in request.history],
                request.model or None,
                registry_key,
                request.diagram_mode,
                request.reasoning_mode,
                request.user_role,
                stats_identity,
                permitted_keys=permitted_keys,
            ):
                # Heartbeats are emitted as SSE comment lines: they keep proxies and
                # load-balancers from dropping the connection during long silent
                # model/tool calls, and are ignored by every SSE consumer (frontend,
                # VS Code extension, MCP/skill parsers all skip non-"data:" lines).
                if event.get("type") == "ping":
                    yield ": keepalive\n\n"
                else:
                    yield f"data: {json.dumps(event)}\n\n"
        except Exception as e:
            error_event = {
                "type": "error",
                "data": {"message": str(e), "traceback": traceback.format_exc()},
            }
            yield f"data: {json.dumps(error_event)}\n\n"

    return StreamingResponse(
        event_stream(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
        },
    )


# ── Tool Execution Endpoint (for MCP server) ────────────────────────
