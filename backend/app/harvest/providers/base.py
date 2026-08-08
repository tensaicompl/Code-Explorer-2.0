"""What every git host adapter has to provide, and the plumbing they share.

A provider answers four questions about a remote host: which repositories exist
under a given owner, what branches a repository has, whether a named branch
exists, and how to authenticate. Everything above this layer is written against
that interface and never against a specific host.

The two pagination helpers are here because the three API-backed hosts differ
only in dialect: Bitbucket pages by start-offset and reports when it is done,
while GitHub and GitLab page by number and signal the end with an empty page.
Both loops are easy to get subtly wrong -- a missing terminating condition
against a large instance means a request loop that never ends -- so they exist
once, with a hard page ceiling as a backstop.
"""
from __future__ import annotations

import fnmatch
import logging
import re
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Iterator, List, Optional, Tuple

import requests
from requests.packages.urllib3.exceptions import InsecureRequestWarning

logger = logging.getLogger("praxevia.harvest.providers")

#: Release branches this scheme understands, e.g. release/1.2.3 or release-x-4.5.
RELEASE_BRANCH_PATTERN = re.compile(r"^release[/\-](?:[a-z]+-)?(\d+(?:\.\d+)*)$")

#: Backoff, in seconds, between attempts when a host answers 429.
_RETRY_BACKOFF_SECONDS = [1, 2, 4]

#: Refuse to page forever, however the host behaves.
_MAX_PAGES = 1000

_REQUEST_TIMEOUT_SECONDS = 30


@dataclass
class RemoteRepo:
    """One repository as the host describes it, before anything is cloned."""

    project_key: str
    slug: str
    clone_url: str
    archived: bool = False
    extra: dict = field(default_factory=dict)


def _slug_matches(slug: str, slug_filter: str) -> bool:
    """Test a repository slug against a comma-separated list of glob patterns.

    An empty pattern in the list is ignored rather than treated as a match-all,
    so a trailing comma cannot silently widen the filter to every repository on
    the host.
    """
    return any(
        pattern.strip() and fnmatch.fnmatch(slug, pattern.strip())
        for pattern in slug_filter.split(",")
    )


def parse_release_version(branch_name: str) -> Optional[tuple]:
    """Turn a release branch name into a comparable version tuple, or None.

    Tuples rather than strings, so release/1.10 correctly sorts above
    release/1.9 -- a string comparison puts them the other way round.
    """
    match = RELEASE_BRANCH_PATTERN.match(branch_name)
    return tuple(int(part) for part in match.group(1).split(".")) if match else None


class RepositoryProvider(ABC):
    """Base for every git host adapter."""

    def __init__(self, base_url: str, token: str, ssl_verify: bool = True):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.session = requests.Session()
        self.session.verify = ssl_verify
        if not ssl_verify:
            # Otherwise every request against a self-signed host prints a
            # warning, and the log becomes unreadable during a large refresh.
            requests.packages.urllib3.disable_warnings(InsecureRequestWarning)

    # -- HTTP ---------------------------------------------------------------

    def _get_with_retry(self, url: str, **kwargs) -> requests.Response:
        """GET, backing off while the host reports rate limiting.

        The host's own Retry-After is preferred over the local backoff schedule
        when it sends one, since it knows when the window actually reopens.
        """
        kwargs.setdefault("timeout", _REQUEST_TIMEOUT_SECONDS)
        response = None
        for attempt, fallback_wait in enumerate(_RETRY_BACKOFF_SECONDS):
            response = self.session.get(url, **kwargs)
            if response.status_code != 429:
                return response
            if attempt < len(_RETRY_BACKOFF_SECONDS) - 1:
                time.sleep(int(response.headers.get("Retry-After", fallback_wait)))
        response.raise_for_status()
        return response

    def _pages_by_offset(self, url: str, params: dict, page_size: int = 1000) -> Iterator[dict]:
        """Walk a Bitbucket-style start/limit collection.

        Yields each page's payload. Stops on isLastPage, on an error, or at the
        page ceiling.
        """
        start = 0
        for _ in range(_MAX_PAGES):
            try:
                response = self._get_with_retry(
                    url, params={**params, "limit": page_size, "start": start}
                )
                response.raise_for_status()
                payload = response.json()
            except Exception as exc:
                logger.warning("paging stopped early at %s: %s", url, exc)
                return

            yield payload

            if payload.get("isLastPage", True):
                return
            start = payload.get("nextPageStart", start + page_size)

    def _pages_by_number(self, url: str, params: dict, page_size: int = 100) -> Iterator[list]:
        """Walk a GitHub/GitLab-style numbered collection.

        Yields each page's list. An empty page ends the walk, which is how both
        hosts signal exhaustion.
        """
        page = 1
        for _ in range(_MAX_PAGES):
            try:
                response = self._get_with_retry(
                    url, params={**params, "per_page": page_size, "page": page}
                )
                response.raise_for_status()
                items = response.json()
            except Exception as exc:
                logger.warning("paging stopped early at %s: %s", url, exc)
                return

            if not items:
                return
            yield items
            page += 1

    # -- interface ----------------------------------------------------------

    @abstractmethod
    def discover_repos(
        self, project_key: str, slug_filter: str = "*"
    ) -> Tuple[List[RemoteRepo], List[RemoteRepo]]:
        """Return (active, archived) repositories matching the slug filter."""

    @abstractmethod
    def list_branches(self, project_key: str, slug: str) -> List[str]:
        """Every branch name on a repository."""

    @abstractmethod
    def check_branch_exists(self, project_key: str, slug: str, branch_name: str) -> bool:
        """Whether one named branch is present."""

    @abstractmethod
    def get_clone_url(self, project_key: str, slug: str) -> str:
        """The URL git should clone from."""

    @abstractmethod
    def get_auth_header(self) -> str:
        """The header git passes as http.extraHeader, or "" for SSH-only hosts."""

    # -- shared behaviour ---------------------------------------------------

    def get_highest_release_branch(self, project_key: str, slug: str) -> Optional[str]:
        """The newest release branch, by version order rather than name order.

        The default implementation lists every branch and filters locally. A
        host whose API can filter server-side should override this -- see the
        Bitbucket adapter, where listing every branch of a large repository is
        several requests that return mostly irrelevant names.
        """
        best_name, best_version = None, None
        for name in self.list_branches(project_key, slug):
            version = parse_release_version(name)
            if version is not None and (best_version is None or version > best_version):
                best_name, best_version = name, version
        return best_name
