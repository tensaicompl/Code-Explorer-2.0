"""The recurring beat that keeps indexed source current.

One job, on a cron expression, refreshing every managed stream. The rest of this
module exists to make that safe to run in more than one process.

Several uvicorn workers each start their own copy of this scheduler, and each
will fire the job at the same instant. A Postgres advisory lock settles it: the
first worker to take the lock does the refresh and the rest return immediately.
The lock is held only for the duration of the run and is released by the server
if the worker dies holding it, so a crashed refresh does not wedge the schedule.

APScheduler is imported lazily and its absence is tolerated. A deployment that
never wants scheduled refreshes should not have to install it, and one that does
gets a clear line in the log rather than a failure to boot.
"""
from __future__ import annotations

import asyncio
import hashlib
import logging
import os
from contextlib import contextmanager

from .db import get_connection
from .harvest import get_refresh_schedule

logger = logging.getLogger("praxevia.cadence")

#: Used when nothing has been configured. Nightly, early enough to be after most
#: CI and well before anyone is reading the results.
DEFAULT_CRON = os.environ.get("REFRESH_CRON", "0 3 * * *")

#: Identifier for the cross-worker lock. Derived from a fixed phrase so every
#: worker computes the same number without needing to agree on one anywhere
#: else. Runtime-only -- nothing persists or compares it between deployments.
_LOCK_ID = int.from_bytes(
    hashlib.sha256(b"praxevia_cadence").digest()[:8], "big", signed=True
)

_JOB_ID = "global_refresh"

_scheduler = None
_scheduler_started = False


def _apscheduler():
    """Return APScheduler's pieces, or None if it is not installed."""
    try:
        from apscheduler.schedulers.asyncio import AsyncIOScheduler
        from apscheduler.triggers.cron import CronTrigger

        return AsyncIOScheduler, CronTrigger
    except ImportError:
        return None


@contextmanager
def _refresh_lock():
    """Hold the cross-worker refresh lock for the duration of the block.

    Yields whether the lock was taken. A worker that did not get it must not do
    the work, and must not release a lock it never held -- releasing another
    worker's lock would let a second refresh start alongside the first.
    """
    acquired = False
    try:
        with get_connection() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT pg_try_advisory_lock(%s)", (_LOCK_ID,))
                acquired = cur.fetchone()[0]
        yield acquired
    finally:
        if acquired:
            try:
                with get_connection() as conn:
                    with conn.cursor() as cur:
                        cur.execute("SELECT pg_advisory_unlock(%s)", (_LOCK_ID,))
            except Exception:
                # Losing the connection releases the lock server-side anyway.
                logger.warning("could not release the refresh lock explicitly")


def _install_job(scheduler, cron_trigger, cron_expression: str) -> bool:
    """Put the refresh job on a scheduler, replacing any previous version.

    max_instances=1 stops a slow refresh from being started again underneath
    itself if the next tick arrives first.
    """
    try:
        trigger = cron_trigger.from_crontab(cron_expression)
    except ValueError:
        logger.error("cron expression %r could not be parsed; leaving the schedule alone",
                     cron_expression)
        return False

    scheduler.add_job(
        _run_scheduled_refresh,
        trigger=trigger,
        id=_JOB_ID,
        replace_existing=True,
        max_instances=1,
    )
    return True


def start_cadence() -> None:
    """Arm the recurring refresh at startup.

    An explicitly disabled schedule is honoured and nothing is armed. No
    schedule at all is treated differently: it falls back to the nightly
    default, because an install nobody has configured otherwise silently drifts
    away from its repositories until somebody notices. Both halves of a refresh
    are incremental -- git pulls, and the indexer re-embeds only what changed --
    so running one nightly costs little.
    """
    global _scheduler, _scheduler_started

    pieces = _apscheduler()
    if pieces is None:
        logger.warning("APScheduler is not installed; scheduled refresh is off")
        return
    scheduler_cls, cron_trigger = pieces

    schedule = get_refresh_schedule()
    if schedule and not schedule.get("enabled"):
        logger.info("the refresh schedule is switched off; nothing armed")
        return

    if schedule and schedule.get("enabled"):
        cron_expression = schedule["cron_expression"]
    else:
        cron_expression = DEFAULT_CRON
        logger.info("no refresh schedule configured; falling back to %s", cron_expression)

    _scheduler = scheduler_cls()
    if not _install_job(_scheduler, cron_trigger, cron_expression):
        return

    _scheduler.start()
    _scheduler_started = True
    logger.info("refresh cadence armed: %s", cron_expression)


def retune_cadence(cron_expression: str, enabled: bool) -> None:
    """Change or silence the schedule without restarting the process."""
    global _scheduler, _scheduler_started

    pieces = _apscheduler()
    if pieces is None:
        return
    scheduler_cls, cron_trigger = pieces

    if _scheduler is None:
        _scheduler = scheduler_cls()

    if _scheduler.get_job(_JOB_ID):
        _scheduler.remove_job(_JOB_ID)

    if not (enabled and cron_expression):
        logger.info("refresh cadence stopped")
        return

    if not _install_job(_scheduler, cron_trigger, cron_expression):
        return

    if not _scheduler_started:
        _scheduler.start()
        _scheduler_started = True
    logger.info("refresh cadence retuned: %s", cron_expression)


async def _run_scheduled_refresh() -> None:
    """What the timer fires. Runs at most once across all workers."""
    with _refresh_lock() as have_lock:
        if not have_lock:
            logger.debug("another worker is running this refresh")
            return
        try:
            from .harvest import sync_all_projects

            logger.info("scheduled refresh starting")
            await asyncio.to_thread(sync_all_projects, "scheduler")
        except Exception:
            logger.exception("scheduled refresh did not complete")


async def _run_off_thread(fn, *args) -> str:
    """Run a blocking refresh without holding up the event loop.

    Both manual triggers below answer an HTTP request, so the caller gets a job
    id back and follows progress through the job record rather than waiting on
    the response.
    """
    try:
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, fn, *args)
    except Exception:
        logger.exception("manually triggered refresh did not complete")
        return ""


async def trigger_full_refresh_async(triggered_by: str = "manual") -> str:
    """Refresh every managed stream now, on request rather than on the timer.

    Deliberately not lock-guarded: somebody pressing the button is asking for it
    to happen now, and harvest.sync already refuses to start a second global
    refresh while one is running.
    """
    from .harvest import sync_all_projects

    return await _run_off_thread(sync_all_projects, triggered_by)


async def trigger_refresh_for_stream_async(stream_id: str, triggered_by: str = "manual") -> str:
    """Refresh one stream now."""
    from .harvest import sync_stream

    return await _run_off_thread(sync_stream, stream_id, triggered_by)
