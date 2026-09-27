"""Azure DevOps adapter, limited to explicitly named repositories over SSH.

Discovery and branch enumeration are not implemented. This is a real limitation
rather than an unfinished file: the deployment this was built for reaches Azure
DevOps over SSH with a deploy key and no API token, so there is nothing to make
API calls with.

The consequence for configuration is that an Azure DevOps source must use
explicit mode with a fixed branch. The unsupported methods raise
NotImplementedError with a message saying exactly that, so a misconfigured
source fails with an actionable error rather than an empty repository list that
looks like a working setup finding nothing.
"""
from __future__ import annotations

from typing import List, Tuple

from .base import RemoteRepo, RepositoryProvider

_ONLY_EXPLICIT = (
    "the Azure DevOps adapter has no API access: configure this source in "
    "explicit mode with a fixed branch"
)


class AzureDevOpsRepositoryProvider(RepositoryProvider):
    def discover_repos(
        self, project_key: str, slug_filter: str = "*"
    ) -> Tuple[List[RemoteRepo], List[RemoteRepo]]:
        raise NotImplementedError(_ONLY_EXPLICIT)

    def list_branches(self, project_key: str, slug: str) -> List[str]:
        raise NotImplementedError(_ONLY_EXPLICIT)

    def check_branch_exists(self, project_key: str, slug: str, branch_name: str) -> bool:
        raise NotImplementedError(_ONLY_EXPLICIT)

    def get_clone_url(self, project_key: str, slug: str) -> str:
        return f"{self.base_url}/{project_key}/_git/{slug}"

    def get_auth_header(self) -> str:
        # Nothing to send: the SSH key is the credential.
        return ""
