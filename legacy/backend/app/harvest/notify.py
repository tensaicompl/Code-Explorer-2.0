"""Telling the indexer that source on disk has changed.

The backend and the indexer are separate processes that share a volume and
nothing else -- no queue, no RPC. The handoff is a single JSON file dropped in a
directory the indexer polls. Crude, but it survives either side restarting and
needs no broker.

The file is overwritten rather than appended to, so a burst of refreshes
collapses into one pending request. That is the behaviour we want: the indexer
reads whatever is there when it next looks, and indexing twice in a row would
achieve nothing.
"""
from __future__ import annotations

import json
import logging
from datetime import datetime
from pathlib import Path
from typing import Optional

from ..config import SIGNAL_DIR

logger = logging.getLogger("praxevia.harvest.notify")

#: The filename the indexer sidecar watches for.
SIGNAL_FILENAME = "refresh-signal.json"


def notify_indexer(project: Optional[str] = None, stream: Optional[str] = None) -> None:
    """Ask the indexer to run.

    Naming both a project and a stream requests just that one; naming neither
    requests a full pass. There is no way to request several specific streams,
    because a single file cannot express a queue -- callers with more than one
    to do should ask for the full pass.

    Failure is logged, not raised: source has already been cloned successfully
    by the time this runs, and losing the notification costs a delay until the
    next scheduled refresh rather than any data.
    """
    scoped = bool(project and stream)
    payload = {
        "action": "index" if scoped else "index_all",
        "timestamp": datetime.utcnow().isoformat(),
    }
    if scoped:
        payload["project"] = project
        payload["stream"] = stream

    try:
        directory = Path(SIGNAL_DIR)
        directory.mkdir(parents=True, exist_ok=True)
        (directory / SIGNAL_FILENAME).write_text(
            json.dumps(payload, indent=2), encoding="utf-8"
        )
    except Exception:
        logger.exception(
            "could not write the indexer signal to %s; indexing will wait for the "
            "next scheduled run",
            SIGNAL_DIR,
        )
        return

    logger.info("asked the indexer to %s", payload["action"])
