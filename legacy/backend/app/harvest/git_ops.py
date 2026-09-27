"""Running git, and deciding which branch to run it against.

Host-agnostic: everything here works through a provider for the questions that
need the remote API, and otherwise just drives the git binary.

All git invocations funnel through `_run_git`, which is what keeps the SSH and
token paths from being written twice. Credentials never appear on the command
line -- a token goes in via `-c http.extraHeader` and an SSH key via
GIT_SSH_COMMAND pointing at a temporary file -- so neither shows up in the
process table where any other user on the host could read it.
"""
from __future__ import annotations

import logging
import os
import shutil
import subprocess
from datetime import datetime
from typing import Optional, Tuple

from .providers import RepositoryProvider
from .ssh import git_ssh_command_for_key, ssh_private_key_tempfile

logger = logging.getLogger("praxevia.harvest.git_ops")

#: A cold clone of a large repository can legitimately take minutes.
CLONE_TIMEOUT_SECONDS = 900

#: A pull has far less to do, so a long stall means something is wrong.
PULL_TIMEOUT_SECONDS = 120

_QUICK_TIMEOUT_SECONDS = 10

#: Branches tried, in order, once the release/develop rules find nothing.
_FALLBACK_BRANCHES = ("main", "master")


def _run_git(args: list, timeout: int, ssh_key: str = "") -> Tuple[bool, str]:
    """Run one git command, returning (ok, output-or-error).

    With an SSH key, the key is written to a temporary file for the duration of
    the call and GIT_SSH_COMMAND points git at it. Without one, the caller has
    already put any token into the argument list as an http.extraHeader
    override.
    """

    def invoke(env=None) -> Tuple[bool, str]:
        try:
            completed = subprocess.run(
                args,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=timeout,
                env=env,
            )
        except subprocess.TimeoutExpired:
            return False, f"git gave up after {timeout}s: {' '.join(args[:2])}"
        except Exception as exc:
            return False, str(exc)

        if completed.returncode == 0:
            return True, completed.stdout.strip()
        return False, completed.stderr.strip()

    if ssh_key:
        with ssh_private_key_tempfile(ssh_key) as key_path:
            return invoke({**os.environ, "GIT_SSH_COMMAND": git_ssh_command_for_key(key_path)})
    return invoke()


# ---------------------------------------------------------------------------
# Branch selection
# ---------------------------------------------------------------------------


def pick_branch(
    provider: RepositoryProvider,
    project_key: str,
    slug: str,
    stream: str,
    strategy: str = "stream",
    fixed_branch: str = "",
) -> Tuple[Optional[str], Optional[str]]:
    """Choose the branch to check out, returning (branch, why) or (None, None).

    Under the "fixed" strategy the configured branch is used if it exists, and
    nothing is substituted if it does not -- silently cloning a different branch
    than an operator pinned would be worse than reporting no branch at all.

    Under the "stream" strategy the stream's name decides the preference order.
    A stream called "develop" wants the develop branch and treats the newest
    release branch as the fallback; any other stream wants the newest release
    branch first. Both then fall back to main, then master.
    """
    if strategy == "fixed" and fixed_branch:
        if provider.check_branch_exists(project_key, slug, fixed_branch):
            return fixed_branch, "fixed"
        return None, None

    if stream == "develop":
        if provider.check_branch_exists(project_key, slug, "develop"):
            return "develop", "develop"
        newest_release = provider.get_highest_release_branch(project_key, slug)
        if newest_release is not None:
            return newest_release, "release"
    else:
        newest_release = provider.get_highest_release_branch(project_key, slug)
        if newest_release is not None:
            return newest_release, "release"
        if provider.check_branch_exists(project_key, slug, "develop"):
            return "develop", "develop"

    for candidate in _FALLBACK_BRANCHES:
        if provider.check_branch_exists(project_key, slug, candidate):
            return candidate, candidate

    return None, None


# ---------------------------------------------------------------------------
# Local checkouts
# ---------------------------------------------------------------------------


def current_branch(dest: str) -> Optional[str]:
    """The branch a checkout is on, or None if it cannot be determined."""
    ok, output = _run_git(
        ["git", "-C", dest, "rev-parse", "--abbrev-ref", "HEAD"],
        _QUICK_TIMEOUT_SECONDS,
    )
    return output if ok else None


def clone_repository(
    clone_url: str,
    branch: str,
    dest: str,
    auth_header: str = "",
    ssh_key: str = "",
) -> Tuple[bool, str]:
    """Clone one branch into a fresh directory.

    --single-branch keeps the checkout to what is actually going to be indexed;
    fetching every branch of every repository would multiply both clone time and
    disk for content nothing reads.
    """
    os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)

    args = ["git", "clone"]
    if not ssh_key:
        args += ["-c", f"http.extraHeader={auth_header}"]
    args += ["--branch", branch, "--single-branch", clone_url, dest]

    ok, output = _run_git(args, CLONE_TIMEOUT_SECONDS, ssh_key=ssh_key)
    if ok:
        return True, f"cloned {branch} into {dest}"
    return False, output


def pull_repository(dest: str, auth_header: str = "", ssh_key: str = "") -> Tuple[bool, str]:
    """Fast-forward an existing checkout.

    --ff-only is deliberate: these checkouts are disposable mirrors that nobody
    commits to, so anything that cannot fast-forward means the remote history
    was rewritten. Failing here surfaces that, where a merge would paper over it
    and index a tree matching neither side.
    """
    if not ssh_key:
        # An extraHeader persisted by an earlier clone would be sent alongside
        # the one below; clearing it first avoids sending a stale token.
        _run_git(
            ["git", "-C", dest, "config", "--unset-all", "http.extraHeader"],
            _QUICK_TIMEOUT_SECONDS,
        )

    args = ["git", "-C", dest]
    if not ssh_key:
        args += ["-c", f"http.extraHeader={auth_header}"]
    args += ["pull", "--ff-only"]

    return _run_git(args, PULL_TIMEOUT_SECONDS, ssh_key=ssh_key)


def remove_local_checkout(dest: str) -> bool:
    """Delete a checkout. False if it could not be removed."""
    try:
        shutil.rmtree(dest)
        return True
    except Exception as exc:
        logger.warning("could not remove %s: %s", dest, exc)
        return False


def sync_local_checkout(
    dest: str,
    clone_url: str,
    desired_branch: str,
    auth_header: str = "",
    ssh_key: str = "",
) -> dict:
    """Bring one repository to the wanted branch, cloning it if it is absent.

    Returns {action, branch, message}, where action is one of cloned, pulled,
    updated or failed. "updated" specifically means the branch changed and the
    checkout was replaced -- a re-clone is used rather than a checkout because
    these are --single-branch clones that do not have the other branch locally.
    """
    if not os.path.exists(dest):
        ok, message = clone_repository(
            clone_url, desired_branch, dest, auth_header=auth_header, ssh_key=ssh_key
        )
        return {"action": "cloned" if ok else "failed", "branch": desired_branch, "message": message}

    existing = current_branch(dest)
    if existing == desired_branch:
        ok, message = pull_repository(dest, auth_header=auth_header, ssh_key=ssh_key)
        return {"action": "pulled" if ok else "failed", "branch": desired_branch, "message": message}

    if not remove_local_checkout(dest):
        return {
            "action": "failed",
            "branch": desired_branch,
            "message": f"{dest} holds {existing or 'an unknown branch'} and could not be removed",
        }

    ok, message = clone_repository(
        clone_url, desired_branch, dest, auth_header=auth_header, ssh_key=ssh_key
    )
    return {
        "action": "updated" if ok else "failed",
        "branch": desired_branch,
        "message": f"{existing or 'unknown'} -> {desired_branch}: {message}",
    }


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------


def write_sync_report(rows: list, path: str) -> None:
    """Write a markdown summary of one pass beside the cloned source.

    Sits in the stream directory rather than in the job log so that whoever is
    looking at the checkout can see how it got there without going back to the
    admin UI.
    """
    lines = [
        "# Repository Sync Report",
        "",
        f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}",
        f"Repositories: {len(rows)}",
        "",
        "| Project | Repository | Branch | Status |",
        "|---------|------------|--------|--------|",
    ]
    lines += [
        f"| {row.get('project', '')} | {row.get('repo', '')} "
        f"| {row.get('branch', '')} | {row.get('status', '')} |"
        for row in rows
    ]

    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(lines) + "\n")
