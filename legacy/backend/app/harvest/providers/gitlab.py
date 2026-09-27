"""GitLab adapter.

GitLab addresses a project by its full namespaced path, URL-encoded into a
single path segment -- "group/sub/repo" becomes "group%2Fsub%2Frepo". Getting
that escaping wrong produces a 404 that reads like a missing repository, so it
is done in one place.

Group discovery includes subgroups, since a GitLab group is usually a tree
rather than a flat list. As with GitHub, an owner may be a group or a user and
the routes differ, so a 404 falls back to the user route.
"""
from __future__ import annotations

from typing import List, Tuple

import requests

from .base import RemoteRepo, RepositoryProvider, _slug_matches


class GitLabRepositoryProvider(RepositoryProvider):
    def __init__(self, base_url: str, token: str, ssl_verify: bool = True):
        super().__init__(base_url, token, ssl_verify)
        self.session.headers["PRIVATE-TOKEN"] = token

    def _project_id(self, project_key: str, slug: str) -> str:
        """The namespaced path, encoded to survive as one URL path segment."""
        return requests.utils.quote(f"{project_key}/{slug}", safe="")

    def _owner_projects_url(self, project_key: str) -> str:
        group_route = f"{self.base_url}/api/v4/groups/{project_key}/projects"
        try:
            probe = self._get_with_retry(group_route, params={"per_page": 1})
            if probe.status_code != 404:
                return group_route
        except Exception:
            return group_route
        return f"{self.base_url}/api/v4/users/{project_key}/projects"

    def discover_repos(
        self, project_key: str, slug_filter: str = "*"
    ) -> Tuple[List[RemoteRepo], List[RemoteRepo]]:
        active: List[RemoteRepo] = []
        archived: List[RemoteRepo] = []

        url = self._owner_projects_url(project_key)
        params = {"include_subgroups": "true"} if "/groups/" in url else {}
        for page in self._pages_by_number(url, params):
            for entry in page:
                slug = entry.get("path", "")
                if not _slug_matches(slug, slug_filter):
                    continue
                repo = RemoteRepo(
                    project_key=project_key,
                    slug=slug,
                    clone_url=entry.get("http_url_to_repo")
                    or self.get_clone_url(project_key, slug),
                    archived=entry.get("archived", False),
                    # Kept so a caller can address the project by numeric id,
                    # which avoids the path-encoding dance entirely.
                    extra={"gitlab_id": entry.get("id")},
                )
                (archived if repo.archived else active).append(repo)

        return active, archived

    def list_branches(self, project_key: str, slug: str) -> List[str]:
        names: List[str] = []
        url = (
            f"{self.base_url}/api/v4/projects/"
            f"{self._project_id(project_key, slug)}/repository/branches"
        )
        for page in self._pages_by_number(url, {}):
            names.extend(entry.get("name", "") for entry in page)
        return names

    def check_branch_exists(self, project_key: str, slug: str, branch_name: str) -> bool:
        try:
            response = self._get_with_retry(
                f"{self.base_url}/api/v4/projects/"
                f"{self._project_id(project_key, slug)}/repository/branches/"
                f"{requests.utils.quote(branch_name, safe='')}"
            )
            return response.status_code == 200
        except Exception:
            return False

    def get_clone_url(self, project_key: str, slug: str) -> str:
        return f"{self.base_url}/{project_key}/{slug}.git"

    def get_auth_header(self) -> str:
        return f"PRIVATE-TOKEN: {self.token}"
