"""Git host adapters, selected by a credential's declared source type.

Adding support for another host means adding a module here, importing its class,
and adding one entry to the registry below. Nothing outside this package needs
to change: callers ask `resolve_provider` for an adapter and then work through
the abstract interface in base.py.
"""
from .azure_devops import AzureDevOpsRepositoryProvider
from .base import RemoteRepo, RepositoryProvider, parse_release_version
from .bitbucket import BitbucketRepositoryProvider
from .github import GitHubRepositoryProvider
from .gitlab import GitLabRepositoryProvider

#: Source type, as stored on a credential, to the adapter that handles it.
_ADAPTERS = {
    "bitbucket": BitbucketRepositoryProvider,
    "github": GitHubRepositoryProvider,
    "gitlab": GitLabRepositoryProvider,
    "azure_devops": AzureDevOpsRepositoryProvider,
}


def resolve_provider(
    source_type: str, base_url: str, token: str, ssl_verify: bool = True
) -> RepositoryProvider:
    """Build the adapter for a source type.

    Raises ValueError for an unknown type, naming the ones that do exist -- the
    value comes from stored configuration, so the reader of this error is an
    operator who needs to know what to put there instead.
    """
    adapter = _ADAPTERS.get(source_type)
    if adapter is None:
        raise ValueError(
            f"unsupported source type {source_type!r}; expected one of "
            + ", ".join(sorted(_ADAPTERS))
        )
    return adapter(base_url, token, ssl_verify)


__all__ = [
    "AzureDevOpsRepositoryProvider",
    "BitbucketRepositoryProvider",
    "GitHubRepositoryProvider",
    "GitLabRepositoryProvider",
    "RemoteRepo",
    "RepositoryProvider",
    "parse_release_version",
    "resolve_provider",
]
