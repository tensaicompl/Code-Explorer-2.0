"""Orchestration for refreshing managed source.

Two entry points, one shape: claim a refresh job, walk one or many streams
through sync_worker, then tell the indexer there is new source to read. The
per-repository work lives in sync_worker; the job bookkeeping lives in schedule;
what is left here is the sequencing and the decision about what counts as a
successful pass.

Both functions return a job id, or an empty string when a refresh of that scope
is already running. That check is advisory rather than a lock -- two workers
starting in the same instant could both pass it -- but the cost is duplicated
work, not corruption, since cloning the same repository twice is idempotent.
"""
from __future__ import annotations

import logging

from .notify import notify_indexer
from .schedule import (
    record_refresh_progress,
    record_stream_refresh_result,
    refresh_in_progress,
    start_refresh_job,
)
from .streams import fetch_stream, list_active_streams
from .sync_worker import sync_stream_repos

logger = logging.getLogger("praxevia.harvest.sync")

#: Progress is reported as 0-90% across cloning, leaving the last tenth for the
#: indexing hand-off so the bar does not sit at 100% while work remains.
_CLONE_PROGRESS_CEILING = 90


def sync_all_projects(triggered_by: str = "scheduler") -> str:
    """Refresh every enabled stream, then ask for a full re-index.

    One failing stream does not stop the others: each is wrapped so the pass
    continues, and the job is marked failed at the end if anything went wrong.
    The indexer is signalled once at the end rather than per stream, since a
    single full pass covers everything that changed.
    """
    if refresh_in_progress(scope="global"):
        logger.warning("a global refresh is already running; leaving it to finish")
        return ""

    job_id = start_refresh_job(
        stream_id=None,
        scope="global",
        trigger_type="scheduled" if triggered_by == "scheduler" else "manual",
        triggered_by=triggered_by,
    )
    record_refresh_progress(job_id, status="running", phase="cloning")

    streams = list_active_streams()
    total_streams = max(len(streams), 1)
    cloned = pulled = failed = seen = 0
    log_lines: list[str] = []

    for position, stream in enumerate(streams):
        label = f"{stream['project_name']}/{stream['stream_name']}"
        record_refresh_progress(
            job_id,
            progress_pct=int((position / total_streams) * _CLONE_PROGRESS_CEILING),
            log_append=f"\n=== {label} ===\n",
        )
        log_lines.append(f"\n=== {label} ===")

        try:
            tally = sync_stream_repos(
                stream["stream_id"],
                stream["project_name"],
                stream["stream_name"],
                log_lines,
                job_id=job_id,
                offset_total=seen,
                offset_cloned=cloned,
                offset_pulled=pulled,
                offset_failed=failed,
            )
        except Exception as exc:
            logger.exception("stream %s could not be refreshed", label)
            log_lines.append(f"  ERROR: {exc}")
            record_stream_refresh_result(stream["stream_id"], "failed", str(exc))
            failed += 1
            record_refresh_progress(
                job_id, repos_failed=failed, log_append=f"  ERROR {label}: {exc}\n"
            )
            continue

        seen += len(tally.rows)
        cloned += tally.cloned
        pulled += tally.pulled
        failed += tally.failed
        record_stream_refresh_result(stream["stream_id"], tally.outcome())

    record_refresh_progress(job_id, phase="indexing", progress_pct=_CLONE_PROGRESS_CEILING)
    notify_indexer()

    record_refresh_progress(
        job_id,
        status="success" if failed == 0 else "failed",
        phase="complete",
        progress_pct=100,
        finished=True,
        log_append=f"\n--- done: {cloned} cloned, {pulled} pulled, {failed} failed ---\n",
    )
    logger.info("global refresh finished: %d cloned, %d pulled, %d failed", cloned, pulled, failed)
    return job_id


def sync_stream(stream_id: str, triggered_by: str = "manual") -> str:
    """Refresh one stream and ask the indexer to re-read just that stream.

    Raises ValueError for an unknown stream id, because that is a caller
    mistake rather than a runtime condition -- unlike a clone failing, which is
    recorded on the job.
    """
    if refresh_in_progress(stream_id=stream_id):
        logger.warning("stream %s is already refreshing; leaving it to finish", stream_id)
        return ""

    stream = fetch_stream(stream_id)
    if not stream:
        raise ValueError(f"no stream with id {stream_id}")

    project_name = stream["project_name"]
    stream_name = stream["stream_name"]

    job_id = start_refresh_job(
        stream_id=stream_id,
        scope="stream",
        trigger_type="manual",
        triggered_by=triggered_by,
    )
    record_refresh_progress(job_id, status="running", phase="cloning")

    log_lines = [f"=== {project_name}/{stream_name} ==="]
    try:
        tally = sync_stream_repos(
            stream_id, project_name, stream_name, log_lines, job_id=job_id
        )
    except Exception as exc:
        logger.exception("stream %s could not be refreshed", stream_id)
        record_refresh_progress(
            job_id,
            status="failed",
            phase="complete",
            finished=True,
            log_append=f"ERROR: {exc}\n",
        )
        record_stream_refresh_result(stream_id, "failed", str(exc))
        return job_id

    record_refresh_progress(
        job_id,
        repos_total=len(tally.rows),
        repos_cloned=tally.cloned,
        repos_pulled=tally.pulled,
        repos_failed=tally.failed,
        log_append="\n".join(log_lines) + "\n",
    )
    record_stream_refresh_result(stream_id, tally.outcome())

    record_refresh_progress(job_id, phase="indexing")
    notify_indexer(project=project_name, stream=stream_name)

    record_refresh_progress(
        job_id,
        status="success" if tally.failed == 0 else "failed",
        phase="complete",
        progress_pct=100,
        finished=True,
    )
    return job_id
