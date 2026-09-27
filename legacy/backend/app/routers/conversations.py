"""Stored transcripts belonging to the calling user.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter, HTTPException, Query, Request

from ..schemas import ConversationRenameRequest, ConversationStartRequest, MessageAppendRequest
from ..storage import conversations
from .guard import ensure_project_access, identify_caller

logger = logging.getLogger("praxevia.routers.conversations")

router = APIRouter()

@router.get("/api/conversations")
async def conversations_list(
    http_request: Request,
    limit: int = Query(default=50, ge=1, le=200),
    offset: int = Query(default=0, ge=0),
):
    user_email = identify_caller(http_request)
    return {"conversations": conversations.list(user_email, limit, offset)}


@router.post("/api/conversations")
async def conversations_create(body: ConversationStartRequest, http_request: Request):
    user_email = identify_caller(http_request)
    if body.project:
        ensure_project_access(user_email, body.project)
    return conversations.create(user_email, body.project, body.stream, body.model)


@router.get("/api/conversations/{conversation_id}")
async def conversations_get(conversation_id: str, http_request: Request):
    user_email = identify_caller(http_request)
    conv = conversations.get(conversation_id, user_email)
    if not conv:
        raise HTTPException(status_code=404, detail="Conversation not found")
    return conv


@router.patch("/api/conversations/{conversation_id}")
async def conversations_update(
    conversation_id: str, body: ConversationRenameRequest, http_request: Request
):
    user_email = identify_caller(http_request)
    if not conversations.rename(conversation_id, user_email, body.title):
        raise HTTPException(status_code=404, detail="Conversation not found")
    return {"ok": True}


@router.delete("/api/conversations/{conversation_id}")
async def conversations_delete(conversation_id: str, http_request: Request):
    user_email = identify_caller(http_request)
    if not conversations.delete(conversation_id, user_email):
        raise HTTPException(status_code=404, detail="Conversation not found")
    return {"ok": True}


# ── API Key Management Endpoints ───────────────────────────────────


@router.post("/api/conversations/{conversation_id}/messages")
async def conversations_append_messages(
    conversation_id: str, body: MessageAppendRequest, http_request: Request
):
    user_email = identify_caller(http_request)
    result = conversations.append_messages(
        conversation_id,
        user_email,
        [m.model_dump() for m in body.messages],
    )
    if not result:
        raise HTTPException(status_code=404, detail="Conversation not found")
    return result


# ── Access Control Endpoints ──────────────────────────────────────────
