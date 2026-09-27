"""Identity and authorization.

Four questions, one package:

  entra.py        who is this, according to the identity provider?
  api_tokens.py   who is this, according to a token they were issued?
  permissions.py  may they see this project?
  admins.py       may they administer the platform?
  requests.py     how do they ask for access they do not have?

Callers should import from this module rather than reaching into the submodules,
so the internal split stays free to move.
"""
from .admins import (
    grant_admin,
    is_platform_admin,
    list_platform_admins,
    revoke_admin,
)
from .api_tokens import (
    API_TOKEN_PREFIX,
    LEGACY_TOKEN_PREFIX,
    ensure_api_token_schema,
    issue_api_token,
    list_api_tokens,
    looks_like_api_token,
    resolve_api_token,
    resolve_api_token_with_name,
    revoke_api_token,
)
from .entra import verify_entra_id_token
from .permissions import (
    ensure_authorization_tables,
    grant_project_permission,
    list_permissions,
    list_permitted_projects,
    revoke_project_permission,
    user_can_access_project,
)
from .requests import (
    approve_request,
    deny_request,
    list_requests,
    list_requests_for_user,
    submit_access_request,
)

__all__ = [
    "API_TOKEN_PREFIX",
    "LEGACY_TOKEN_PREFIX",
    "approve_request",
    "deny_request",
    "ensure_api_token_schema",
    "ensure_authorization_tables",
    "grant_admin",
    "grant_project_permission",
    "is_platform_admin",
    "issue_api_token",
    "list_api_tokens",
    "list_permissions",
    "list_permitted_projects",
    "list_platform_admins",
    "list_requests",
    "list_requests_for_user",
    "looks_like_api_token",
    "resolve_api_token",
    "resolve_api_token_with_name",
    "revoke_admin",
    "revoke_api_token",
    "revoke_project_permission",
    "submit_access_request",
    "user_can_access_project",
    "verify_entra_id_token",
]
