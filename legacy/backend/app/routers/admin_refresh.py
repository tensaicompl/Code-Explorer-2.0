"""Triggering refreshes, watching them run, and setting the schedule.
"""
from __future__ import annotations

import asyncio
import logging

from fastapi import APIRouter, HTTPException, Query, Request

from ..cadence import retune_cadence, trigger_full_refresh_async, trigger_refresh_for_stream_async
from ..harvest import fetch_refresh_job, get_refresh_schedule, list_refresh_job_history, set_refresh_schedule
from ..schemas import ScheduleUpdateRequest
from .guard import ensure_admin, identify_caller, to_json_safe

logger = logging.getLogger("praxevia.routers.admin_refresh")

router = APIRouter()

@router.post("/api/admin/refresh/global")
async def admin_trigger_global_refresh(http_request: Request):
    from ..harvest import refresh_in_progress
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if refresh_in_progress(scope="global"):
        raise HTTPException(status_code=409, detail="A global refresh is already running")
    asyncio.ensure_future(trigger_full_refresh_async(user_identity))
    return {"ok": True, "message": "Global refresh started"}


@router.post("/api/admin/refresh/stream/{stream_id}")
async def admin_trigger_stream_refresh(stream_id: str, http_request: Request):
    from ..harvest import refresh_in_progress
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    if refresh_in_progress(stream_id=stream_id):
        raise HTTPException(status_code=409, detail="A refresh is already running for this stream")
    asyncio.ensure_future(trigger_refresh_for_stream_async(stream_id, user_identity))
    return {"ok": True, "message": "Stream refresh started"}


@router.get("/api/admin/refresh-jobs")
async def admin_list_refresh_jobs(
    http_request: Request,
    stream_id: str = Query(default=None),
    limit: int = Query(default=50),
):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    jobs = list_refresh_job_history(stream_id=stream_id, limit=limit)
    return {"jobs": [to_json_safe(j) for j in jobs]}


@router.get("/api/admin/refresh-jobs/{job_id}")
async def admin_get_refresh_job(job_id: str, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    job = fetch_refresh_job(job_id)
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return {"job": to_json_safe(job)}


@router.get("/api/admin/schedule")
async def admin_get_schedule(http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    schedule = get_refresh_schedule()
    return {"schedule": to_json_safe(schedule) if schedule else None}


@router.put("/api/admin/schedule")
async def admin_set_schedule(body: ScheduleUpdateRequest, http_request: Request):
    user_identity = identify_caller(http_request)
    ensure_admin(user_identity)
    result = set_refresh_schedule(body.cron_expression, body.enabled, user_identity)
    retune_cadence(body.cron_expression, body.enabled)
    return {"ok": True, "schedule": to_json_safe(result)}
