"""Bitbucket Data Center adapter.

Authenticates with a bearer token and pages with start/limit offsets. Clone URLs
are built rather than read from the payload, because the /scm/ path is
predictable and the API's own href list varies by instance configuration.
"""
from __future__ import annotations

from typing import List, Optional, Tuple

from .base import RemoteRepo, RepositoryProvider, _slug_matches, parse_release_version


class BitbucketRepositoryProvider(RepositoryProvider):
    def __init__(self, base_url: str, token: str, ssl_verify: bool = True):
        super().__init__(base_url, token, ssl_verify)
        self.session.headers["Authorization"] = f"Bearer {token}"

    def _repo_url(self, project_key: str, slug: str = "") -> str:
        base = f"{self.base_url}/rest/api/1.0/projects/{project_key}/repos"
        return f"{base}/{slug}" if slug else base

    def discover_repos(
        self, project_key: str, slug_filter: str = "*"
    ) -> Tuple[List[RemoteRepo], List[RemoteRepo]]:
        active: List[RemoteRepo] = []
        archived: List[RemoteRepo] = []

        for page in self._pages_by_offset(self._repo_url(project_key), {}):
            for entry in page.get("values", []):
                slug = entry.get("slug", "")
                if not _slug_matches(slug, slug_filter):
                    continue
                repo = RemoteRepo(
                    project_key=project_key,
                    slug=slug,
                    clone_url=self.get_clone_url(project_key, slug),
                    archived=entry.get("archived", False),
                )
                (archived if repo.archived else active).append(repo)

        return active, archived

    def list_branches(self, project_key: str, slug: str) -> List[str]:
        names: List[str] = []
        for page in self._pages_by_offset(f"{self._repo_url(project_key, slug)}/branches", {}):
            names.extend(entry.get("displayId", "") for entry in page.get("values", []))
        return names

    def check_branch_exists(self, project_key: str, slug: str, branch_name: str) -> bool:
        try:
            response = self._get_with_retry(
                f"{self._repo_url(project_key, slug)}/branches",
                params={"filterText": branch_name, "limit": 25},
            )
            response.raise_for_status()
            values = response.json().get("values", [])
        except Exception:
            return False
        # filterText is a substring match, so an exact comparison is still
        # needed -- asking for "main" also returns "maintenance".
        return any(entry.get("displayId") == branch_name for entry in values)

    def get_highest_release_branch(self, project_key: str, slug: str) -> Optional[str]:
        """Overridden to filter server-side.

        Bitbucket can narrow branches to those containing "release", which on a
        repository with hundreds of feature branches turns several pages of
        mostly-irrelevant names into one.
        """
        best_name, best_version = None, None
        pages = self._pages_by_offset(
            f"{self._repo_url(project_key, slug)}/branches", {"filterText": "release"}
        )
        for page in pages:
            for entry in page.get("values", []):
                name = entry.get("displayId", "")
                version = parse_release_version(name)
                if version is not None and (best_version is None or version > best_version):
                    best_name, best_version = name, version
        return best_name

    def get_clone_url(self, project_key: str, slug: str) -> str:
        return f"{self.base_url}/scm/{project_key.lower()}/{slug}.git"

    def get_auth_header(self) -> str:
        return f"Authorization: Bearer {self.token}"
