"""The HTTP surface, one module per area of the API.

  guard.py              authentication and the shared access checks (no routes)
  auth.py               signing in
  health.py             liveness
  projects.py           what the caller may explore
  chat.py               the agent, streamed
  tool_exec.py          one search tool, without the agent
  conversations.py      stored transcripts
  access_keys.py        user-issued API tokens
  identity.py           who am I, and requesting access
  admin_access.py       permissions, requests, admin roster
  admin_credentials.py  git host credentials
  admin_projects.py     managed projects
  admin_streams.py      streams and repository sources
  admin_refresh.py      refreshes and the schedule
  preferences.py        per-user UI state

`ATTACH_IN_ORDER` is what main.py iterates. Order is not arbitrary: FastAPI
matches routes in registration order, so a router declaring a literal path must
be attached before one declaring a parameterised path that would also match it.
"""
from . import (
    access_keys,
    admin_access,
    admin_credentials,
    admin_projects,
    admin_refresh,
    admin_streams,
    auth,
    chat,
    conversations,
    health,
    identity,
    preferences,
    projects,
    tool_exec,
)

#: Every routed module, in the order main.py attaches them.
ATTACH_IN_ORDER = (
    auth,
    health,
    projects,
    chat,
    tool_exec,
    conversations,
    access_keys,
    identity,
    admin_access,
    admin_credentials,
    admin_projects,
    admin_streams,
    admin_refresh,
    preferences,
)

__all__ = ["ATTACH_IN_ORDER"]
