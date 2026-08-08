"""Adopting source that is already on the volume.

Content can arrive under CODEBASE_DIR without ever having been configured --
placed there by hand, or left over from before the managed-project tables
existed. build_workspace_registry already makes such directories searchable as
"legacy" entries, but they cannot be scheduled, given sources, or refreshed
because no project record describes them.

This turns that directory tree into records. It only ever inserts: an existing
project or stream is left exactly as it is, so running it twice is safe and a
directory that was deliberately reconfigured is not reverted.

Adopted projects are created in "manual" mode rather than "managed". They have
no repository source to clone from, and marking them managed would enroll them
in refreshes that would find nothing to do.
"""
from __future__ import annotations

import logging

from ..config import CODEBASE_DIR
from ..db import transaction
from .registry import IGNORED_DIRS

logger = logging.getLogger("praxevia.harvest.migrate")


def import_filesystem_projects(created_by: str) -> int:
    """Create records for unconfigured directories. Returns how many streams were added.

    The whole scan runs in one transaction, so a failure part-way through leaves
    no half-adopted project with some of its streams registered.
    """
    if not CODEBASE_DIR.exists():
        logger.info("nothing to adopt: %s does not exist", CODEBASE_DIR)
        return 0

    adopted = 0
    with transaction(dict_rows=True) as cur:
        for project_dir in sorted(CODEBASE_DIR.iterdir()):
            if not project_dir.is_dir() or project_dir.name in IGNORED_DIRS:
                continue

            cur.execute(
                "SELECT id FROM managed_projects WHERE name = %s", (project_dir.name,)
            )
            found = cur.fetchone()
            if found:
                project_id = found["id"]
            else:
                cur.execute(
                    """
                    INSERT INTO managed_projects (name, display_name, mode, created_by)
                    VALUES (%s, %s, 'manual', %s) RETURNING id
                    """,
                    (project_dir.name, project_dir.name, created_by),
                )
                project_id = cur.fetchone()["id"]
                logger.info("adopted project %s from disk", project_dir.name)

            for stream_dir in sorted(project_dir.iterdir()):
                if not stream_dir.is_dir() or stream_dir.name in IGNORED_DIRS:
                    continue
                cur.execute(
                    "SELECT id FROM project_streams WHERE project_id = %s AND stream_name = %s",
                    (project_id, stream_dir.name),
                )
                if cur.fetchone():
                    continue
                cur.execute(
                    "INSERT INTO project_streams (project_id, stream_name) VALUES (%s, %s)",
                    (project_id, stream_dir.name),
                )
                adopted += 1

    logger.info("adopted %d stream(s) from %s", adopted, CODEBASE_DIR)
    return adopted
