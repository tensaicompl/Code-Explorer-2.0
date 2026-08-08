"""The per-stream cloning loop.

One stream can be fed by several sources, and a source is either a *wildcard*
(discover every repository under a hosting project, filtered by slug) or
*explicit* (one named repository). Both end at the same place -- a local
checkout on the right branch -- so the two branches below differ only in how
they arrive at a list of repositories to fetch.

Nothing here raises for a single repository failing. A stream feeding forty
repositories should index the thirty-nine that worked, so failures are counted
and written into the job log instead of aborting the pass.
"""
from __future__ import annotations

import logging
import os
from dataclasses import dataclass, field
from typing import Optional

from ..config import CODEBASE_DIR
from ..storage import fetch_credential_with_secret
from .git_ops import pick_branch, sync_local_checkout, write_sync_report
from .providers import resolve_provider
from .schedule import record_refresh_progress
from .streams import list_repository_sources

logger = logging.getLogger("praxevia.harvest.sync_worker")

#: Written into each stream directory after a pass, for operators to read.
SYNC_REPORT_FILENAME = "clone-report.md"


@dataclass
class SyncTally:
    """Running totals for one pass, plus the rows the report is built from.

    Carrying the counts in one object is what lets both source branches share a
    single progress-reporting path. `offset_*` hold the totals accumulated by
    earlier streams in a global refresh, so the job's progress bar counts
    across the whole run rather than restarting at each stream.
    """

    rows: list = field(default_factory=list)
    cloned: int = 0
    pulled: int = 0
    failed: int = 0

    offset_total: int = 0
    offset_cloned: int = 0
    offset_pulled: int = 0
    offset_failed: int = 0

    def record(self, result: dict) -> None:
        """File one repository's outcome under the right counter."""
        self.rows.append(result)
        action = result.get("action")
        if action == "cloned":
            self.cloned += 1
        elif action in ("pulled", "updated"):
            self.pulled += 1
        elif action == "failed":
            self.failed += 1

    def as_totals(self) -> dict:
        """Cumulative counts, for writing onto the refresh job."""
        return {
            "repos_total": self.offset_total + len(self.rows),
            "repos_cloned": self.offset_cloned + self.cloned,
            "repos_pulled": self.offset_pulled + self.pulled,
            "repos_failed": self.offset_failed + self.failed,
        }

    def outcome(self) -> str:
        """A stream counts as failed only if nothing at all succeeded."""
        return "failed" if self.failed and not (self.cloned or self.pulled) else "success"


def infer_repo_slug(url: str) -> str:
    """Derive a directory name from a clone URL when none was configured."""
    name = url.rstrip("/").rsplit("/", 1)[-1]
    return name[:-4] if name.endswith(".git") else name


def _note(tally: SyncTally, log_lines: list, job_id: Optional[str], line: str) -> None:
    """Append one line to both the in-memory log and the stored job record."""
    log_lines.append(line)
    if job_id:
        record_refresh_progress(job_id, log_append=line + "\n", **tally.as_totals())


def _fetch_one(
    provider,
    tally: SyncTally,
    log_lines: list,
    job_id: Optional[str],
    *,
    dest: str,
    clone_url: str,
    owner: str,
    slug: str,
    stream_name: str,
    branch_strategy: str,
    fixed_branch: str,
    auth_header: str,
    ssh_key: str,
    known_branch: Optional[str] = None,
) -> None:
    """Resolve a branch, clone or pull, and record the result.

    `known_branch` short-circuits branch resolution. It is set only for an
    explicit SSH source pinned to a fixed branch, where the provider API cannot
    be queried for a branch list because there is no API token to query it with.
    """
    branch = known_branch
    if branch is None:
        branch, _source = pick_branch(
            provider, owner, slug, stream_name, branch_strategy, fixed_branch
        )
        if branch is None:
            tally.record(
                {
                    "project": owner,
                    "repo": slug,
                    "branch": "",
                    "status": "NO BRANCH",
                    "action": "no_branch",
                }
            )
            _note(tally, log_lines, job_id, f"  NO BRANCH {slug}")
            return

    result = sync_local_checkout(
        dest, clone_url, branch, auth_header=auth_header, ssh_key=ssh_key
    )
    result["project"] = owner
    result["repo"] = slug
    result["status"] = result["action"].upper()
    tally.record(result)
    _note(
        tally,
        log_lines,
        job_id,
        f"  {result['action'].upper():8s} {slug} @ {result.get('branch', '?')}",
    )


def sync_stream_repos(
    stream_id: str,
    project_name: str,
    stream_name: str,
    log_lines: list,
    job_id: Optional[str] = None,
    offset_total: int = 0,
    offset_cloned: int = 0,
    offset_pulled: int = 0,
    offset_failed: int = 0,
) -> SyncTally:
    """Bring every repository backing one stream up to date.

    Returns the tally. Sources without a usable credential are skipped with a
    note rather than counted as failures -- an unconfigured source is an
    administrative gap, not a fetch that went wrong.
    """
    tally = SyncTally(
        offset_total=offset_total,
        offset_cloned=offset_cloned,
        offset_pulled=offset_pulled,
        offset_failed=offset_failed,
    )
    base_dir = str(CODEBASE_DIR / project_name / stream_name)

    for source in list_repository_sources(stream_id):
        credential_id = source.get("credential_id")
        if not credential_id:
            log_lines.append(f"  SKIP source {source['id']}: no credential assigned")
            continue

        credential = fetch_credential_with_secret(credential_id)
        if not credential:
            log_lines.append(f"  SKIP source {source['id']}: credential no longer exists")
            continue

        uses_ssh = credential.get("auth_type", "bearer_token") == "ssh_key"
        ssh_key = credential.get("ssh_key", "") if uses_ssh else ""

        provider = resolve_provider(
            credential["source_type"],
            credential["base_url"],
            credential["token"] or "",
            credential["ssl_verify"],
        )
        # With an SSH key the transport carries the credential, so no HTTP
        # Authorization header is sent -- and must not be, since there is no
        # token to put in it.
        auth_header = "" if ssh_key else provider.get_auth_header()

        branch_strategy = source.get("branch_strategy", "stream")
        fixed_branch = source.get("fixed_branch", "")

        if source["source_mode"] == "wildcard":
            hosting_key = source.get("hosting_project_key", "")
            if not hosting_key:
                log_lines.append(f"  SKIP source {source['id']}: no hosting project key")
                continue

            try:
                discovered, _archived = provider.discover_repos(
                    hosting_key, source.get("slug_filter", "*")
                )
            except Exception as exc:
                log_lines.append(f"  ERROR listing repositories under {hosting_key}: {exc}")
                tally.failed += 1
                continue

            for repo in discovered:
                _fetch_one(
                    provider,
                    tally,
                    log_lines,
                    job_id,
                    dest=os.path.join(base_dir, hosting_key, repo.slug),
                    clone_url=repo.clone_url,
                    owner=hosting_key,
                    slug=repo.slug,
                    stream_name=stream_name,
                    branch_strategy=branch_strategy,
                    fixed_branch=fixed_branch,
                    auth_header=auth_header,
                    ssh_key=ssh_key,
                )

        elif source["source_mode"] == "explicit":
            repo_url = source.get("repo_url", "")
            if not repo_url:
                log_lines.append(f"  SKIP source {source['id']}: no repository URL")
                continue

            slug = source.get("repo_slug", "") or infer_repo_slug(repo_url)
            pinned = fixed_branch if (ssh_key and branch_strategy == "fixed" and fixed_branch) else None
            _fetch_one(
                provider,
                tally,
                log_lines,
                job_id,
                dest=os.path.join(base_dir, slug),
                clone_url=repo_url,
                owner=project_name,
                slug=slug,
                stream_name=stream_name,
                branch_strategy=branch_strategy,
                fixed_branch=fixed_branch,
                auth_header=auth_header,
                ssh_key=ssh_key,
                known_branch=pinned,
            )

    if tally.rows:
        write_sync_report(tally.rows, os.path.join(base_dir, SYNC_REPORT_FILENAME))

    return tally
