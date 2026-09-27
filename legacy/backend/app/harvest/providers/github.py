"""GitHub adapter, covering both github.com and GitHub Enterprise.

The two differ only in where the API lives: github.com serves it from a separate
host, while Enterprise serves it under /api/v3 on the instance itself. That is
settled once in the constructor.

An owner may be an organisation or a user, and the API keeps those on different
routes with no way to ask which one applies. Discovery therefore tries the
organisation route and falls back to the user route on a 404.
"""
from __future__ import annotations

from typing import List, Tuple

from .base import RemoteRepo, RepositoryProvider, _slug_matches


class GitHubRepositoryProvider(RepositoryProvider):
    def __init__(self, base_url: str, token: str, ssl_verify: bool = True):
        super().__init__(base_url, token, ssl_verify)
        self.session.headers["Authorization"] = f"token {token}"
        self.session.headers["Accept"] = "application/vnd.github+json"
        self.api_base = (
            "https://api.github.com"
            if "github.com" in base_url
            else f"{self.base_url}/api/v3"
        )

    def _owner_repos_url(self, project_key: str) -> str:
        """The repository-listing route for this owner, org or user.

        One probe request decides which; the answer then serves the whole
        paginated walk rather than being rediscovered per page.
        """
        org_route = f"{self.api_base}/orgs/{project_key}/repos"
        try:
            probe = self._get_with_retry(org_route, params={"per_page": 1})
            if probe.status_code != 404:
                return org_route
        except Exception:
            return org_route
        return f"{self.api_base}/users/{project_key}/repos"

    def discover_repos(
        self, project_key: str, slug_filter: str = "*"
    ) -> Tuple[List[RemoteRepo], List[RemoteRepo]]:
        active: List[RemoteRepo] = []
        archived: List[RemoteRepo] = []

        pages = self._pages_by_number(self._owner_repos_url(project_key), {"type": "all"})
        for page in pages:
            for entry in page:
                slug = entry.get("name", "")
                if not _slug_matches(slug, slug_filter):
                    continue
                repo = RemoteRepo(
                    project_key=project_key,
                    slug=slug,
                    clone_url=entry.get("clone_url") or self.get_clone_url(project_key, slug),
                    archived=entry.get("archived", False),
                )
                (archived if repo.archived else active).append(repo)

        return active, archived

    def list_branches(self, project_key: str, slug: str) -> List[str]:
        names: List[str] = []
        for page in self._pages_by_number(
            f"{self.api_base}/repos/{project_key}/{slug}/branches", {}
        ):
            names.extend(entry.get("name", "") for entry in page)
        return names

    def check_branch_exists(self, project_key: str, slug: str, branch_name: str) -> bool:
        try:
            response = self._get_with_retry(
                f"{self.api_base}/repos/{project_key}/{slug}/branches/{branch_name}"
            )
            return response.status_code == 200
        except Exception:
            return False

    def get_clone_url(self, project_key: str, slug: str) -> str:
        return f"{self.base_url}/{project_key}/{slug}.git"

    def get_auth_header(self) -> str:
        return f"Authorization: token {self.token}"
