"""Request bodies accepted by the HTTP API.

Everything here is a Pydantic model used purely to validate and coerce an
incoming JSON body. There is no persistence, no business logic and no response
shaping -- responses are assembled as plain dicts at the route layer, so adding
a model here does not oblige anyone to use it on the way back out.

Field names are a published contract. They are what the dashboard sends and, for
PreferencesPayload, what is stored verbatim as JSONB keys in user_preferences.
Renaming a class is free; renaming a field silently breaks the frontend and
orphans every row already written. Class names may be reshaped at will, field
names may not.
"""
from __future__ import annotations

from typing import List, Optional

from pydantic import BaseModel, model_validator


# ===========================================================================
# Conversation and chat
# ===========================================================================


class ChatTurn(BaseModel):
    """One exchange in a transcript. `role` is either "user" or "assistant"."""

    role: str
    content: str


class ChatPromptRequest(BaseModel):
    """A question plus the full context the agent needs to answer it.

    `history` replays prior turns; the caller owns the transcript, the server
    keeps no implicit session. The jira/confluence tokens are per-request
    credentials supplied by the browser and are never persisted.
    """

    message: str
    history: List[ChatTurn] = []
    model: str = ""
    project: str = ""
    stream: str = ""
    cross_project: bool = False
    diagram_mode: str = "mermaid"
    reasoning_mode: str = "low-level"
    user_role: str = "general"
    jira_token: str = ""
    confluence_token: str = ""


class ConversationStartRequest(BaseModel):
    """Opens a stored conversation. Title is derived server-side from the
    first message, so it is deliberately absent here."""

    project: str = ""
    stream: str = ""
    model: str = ""


class ConversationRenameRequest(BaseModel):
    title: str


class MessageAppendRequest(BaseModel):
    """Appends turns to an existing conversation, in order."""

    messages: List[ChatTurn]


# ===========================================================================
# Agent tooling
# ===========================================================================


class ToolInvocationRequest(BaseModel):
    """Runs one search tool directly, bypassing the agent loop.

    This is the entry point the MCP surface posts to. `tool_name` is validated
    against the exposed tool list before dispatch -- an unrecognised name is
    rejected rather than passed through.
    """

    tool_name: str
    tool_input: dict = {}
    project: str
    stream: str
    cross_project: bool = False


# ===========================================================================
# Identity, permissions and API tokens
# ===========================================================================


class ApiTokenCreateRequest(BaseModel):
    """`name` is a human label for the token, shown in the token list. The
    secret itself is generated server-side and returned exactly once."""

    name: str


class AccessRequestSubmission(BaseModel):
    """A user asking for access to a project they cannot currently see."""

    project: str


class PermissionGrantRequest(BaseModel):
    user_email: str
    project: str


class AdminGrantRequest(BaseModel):
    user_email: str


# ===========================================================================
# Per-user settings
# ===========================================================================


class PreferencesPayload(BaseModel):
    """UI state persisted per user.

    Every field is optional: the client sends a partial object and omitted keys
    keep their stored value. These names are camelCase because they are stored
    as-is in a JSONB column and read straight back by the dashboard -- do not
    normalise them to snake_case.
    """

    selectedModel: Optional[str] = None
    diagramMode: Optional[str] = None
    reasoningMode: Optional[str] = None
    userRole: Optional[str] = None
    selectedProject: Optional[str] = None
    selectedStream: Optional[str] = None


# ===========================================================================
# Workspace registry: projects, streams and their repository sources
# ===========================================================================


class ProjectCreateRequest(BaseModel):
    """`mode` selects how the project is populated -- "managed" means the
    refresh machinery clones and tracks it on a schedule."""

    name: str
    display_name: str = ""
    mode: str = "managed"


class ProjectUpdateRequest(BaseModel):
    display_name: Optional[str] = None
    mode: Optional[str] = None


class StreamCreateRequest(BaseModel):
    stream_name: str


class StreamUpdateRequest(BaseModel):
    """Only the enabled flag is mutable; a stream's name is its identity."""

    enabled: Optional[bool] = None


class RepositorySourceCreateRequest(BaseModel):
    """Attaches a remote to a stream.

    Two shapes share this model. A wildcard source discovers many repositories
    from a hosting project and filters them with `slug_filter`; a single source
    names one `repo_url`/`repo_slug` outright. `branch_strategy` decides whether
    the checked-out branch follows the stream name or the pinned
    `fixed_branch`.
    """

    source_mode: str
    credential_id: Optional[str] = None
    hosting_project_key: str = ""
    slug_filter: str = ""
    repo_url: str = ""
    repo_slug: str = ""
    branch_strategy: str = "stream"
    fixed_branch: str = ""


class RepositorySourceUpdateRequest(BaseModel):
    """Partial edit of an attached source. `source_mode` is absent on purpose:
    switching a wildcard source to a single one is a different source, so it is
    detached and re-attached rather than mutated."""

    credential_id: Optional[str] = None
    hosting_project_key: Optional[str] = None
    slug_filter: Optional[str] = None
    repo_url: Optional[str] = None
    repo_slug: Optional[str] = None
    branch_strategy: Optional[str] = None
    fixed_branch: Optional[str] = None


# ===========================================================================
# Git hosting credentials
# ===========================================================================


class CredentialCreateRequest(BaseModel):
    """Registers a secret used to reach a git host.

    Exactly one kind of secret is meaningful per credential, decided by
    `auth_type`. The validator below enforces that the matching field is
    actually populated, because an empty secret would otherwise be accepted
    here and only fail much later, mid-clone, as an opaque auth error.
    """

    name: str
    source_type: str
    base_url: str
    auth_type: str = "bearer_token"
    token: Optional[str] = None
    ssh_key: Optional[str] = None
    ssl_verify: bool = True

    @model_validator(mode="after")
    def _require_matching_secret(self):
        if self.auth_type == "ssh_key" and not self.ssh_key:
            raise ValueError("auth_type 'ssh_key' needs an ssh_key value")
        if self.auth_type == "bearer_token" and not self.token:
            raise ValueError("auth_type 'bearer_token' needs a token value")
        return self


class CredentialUpdateRequest(BaseModel):
    """Partial edit. Omitting `token`/`ssh_key` leaves the stored secret in
    place -- it is never returned to the client, so the client cannot echo it
    back and has no way to resend it unchanged."""

    name: Optional[str] = None
    base_url: Optional[str] = None
    auth_type: Optional[str] = None
    token: Optional[str] = None
    ssh_key: Optional[str] = None
    ssl_verify: Optional[bool] = None


# ===========================================================================
# Refresh scheduling
# ===========================================================================


class ScheduleUpdateRequest(BaseModel):
    """Sets the recurring refresh cadence. `cron_expression` is parsed by the
    scheduler, which rejects it there rather than here."""

    cron_expression: str
    enabled: bool = True
