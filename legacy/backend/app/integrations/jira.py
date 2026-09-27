"""Pulls Jira issue detail into a chat message.

Same bargain as the Confluence integration: a linked issue is fetched and its
summary, status, description, acceptance criteria and recent comments are
appended to the message, so a question like "why is this failing?" can be
answered against the ticket rather than against its URL.

READ-ONLY BY CONSTRUCTION. Every request goes through `_READONLY_CLIENT` and the
only method ever called on it is .get(). Nothing in this module can create,
transition or comment on an issue, and that is a property to preserve: this runs
with a service token that may well have write access.

Which tracker counts as "ours" comes from JIRA_SITE_URL. Unset means the link
pattern matches nothing and enrichment does not run.
"""
from __future__ import annotations

import logging
import re
from typing import List, Optional

import httpx

from .. import config
from .rendering import append_context_section

logger = logging.getLogger("praxevia.integrations.jira")

#: Issue links, in either the short /browse/KEY-1 form or the longer
#: /projects/KEY/issues/KEY-1 form the issue navigator produces.
JIRA_ISSUE_LINK_RE = (
    re.compile(
        re.escape(config.JIRA_SITE_URL)
        + r"/(?:browse|projects/[A-Za-z]+/issues)/([A-Z]+-\d+)"
    )
    if config.JIRA_SITE_URL
    else None
)

#: A GET-only client. Certificate verification follows JIRA_VERIFY_SSL, which is
#: off by default because self-hosted trackers frequently present a chain the
#: container does not trust; a deployment with a proper certificate should turn
#: it on.
_READONLY_CLIENT = httpx.Client(verify=config.JIRA_VERIFY_SSL, timeout=10.0)

#: How many trailing comments to include. Enough for recent context without
#: crowding out the codebase content the question is actually about.
_RECENT_COMMENT_LIMIT = 5

#: Where this Jira keeps acceptance criteria. Custom fields are per-instance, so
#: a miss here silently falls back to parsing the description.
_ACCEPTANCE_FIELD = "customfield_10206"


def find_jira_issue_keys(text: str) -> List[str]:
    """Every distinct issue key linked in the text, in order of appearance."""
    if JIRA_ISSUE_LINK_RE is None:
        return []
    return list(dict.fromkeys(JIRA_ISSUE_LINK_RE.findall(text)))


def fetch_issue(issue_key: str, token: str) -> Optional[dict]:
    """Retrieve one issue, or None if it cannot be read.

    Failures return None rather than raising, so an unreachable tracker costs
    the user context but not their question.
    """
    if not config.JIRA_SITE_URL:
        return None

    try:
        response = _READONLY_CLIENT.get(
            f"{config.JIRA_SITE_URL}/rest/api/2/issue/{issue_key}",
            headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
        )
    except Exception:
        logger.exception("could not reach Jira for issue %s", issue_key)
        return None

    if response.status_code == 401:
        logger.warning("Jira rejected the token while reading %s", issue_key)
        return None
    if response.status_code == 404:
        logger.warning("Jira has no issue %s, or the token cannot see it", issue_key)
        return None

    try:
        response.raise_for_status()
        return response.json()
    except Exception:
        logger.exception("unreadable Jira response for issue %s", issue_key)
        return None


def _acceptance_criteria(fields: dict, description: str) -> str:
    """Find acceptance criteria in the custom field, else in the description.

    Plenty of teams type the criteria into the description under a heading
    instead of filling the field in, so falling back to a scan of the prose
    recovers them for those issues.
    """
    stated = fields.get(_ACCEPTANCE_FIELD) or ""
    if stated:
        return stated
    if not description:
        return ""

    match = re.search(
        r"(?:acceptance criteria|AC)[:\s]*\n(.*?)(?:\n\n|\Z)",
        description,
        re.IGNORECASE | re.DOTALL,
    )
    return match.group(1).strip() if match else ""


def _render_issue_block(issue_key: str, data: dict) -> str:
    """Turn an issue payload into the markdown the model will read."""
    fields = data.get("fields", {})
    description = fields.get("description") or "_This issue has no description._"

    lines = [
        f"## Jira Issue: {issue_key}",
        f"**Summary**: {fields.get('summary', 'none given')}",
        f"**Status**: {(fields.get('status') or {}).get('name', 'unknown')}"
        f" | **Priority**: {(fields.get('priority') or {}).get('name', 'unset')}",
        f"**Labels**: {', '.join(fields.get('labels', [])) or 'none'}"
        f" | **Components**: "
        f"{', '.join(c.get('name', '') for c in fields.get('components', [])) or 'none'}",
        "",
        "**Description**:",
        description,
    ]

    criteria = _acceptance_criteria(fields, description)
    if criteria:
        lines += ["", "**Acceptance Criteria**:", criteria]

    thread = fields.get("comment", {})
    comments = thread.get("comments", [])
    recent = comments[-_RECENT_COMMENT_LIMIT:]
    if recent:
        total = thread.get("total", len(comments))
        lines += ["", f"**Latest Comments** ({len(recent)} shown of {total}):"]
        for comment in recent:
            author = (comment.get("author") or {}).get("displayName", "unknown author")
            posted = (comment.get("created") or "")[:10]
            lines.append(f"*{author}* ({posted}): {comment.get('body', '')}")

    return "\n".join(lines)


def enrich_with_jira_context(message: str, token: Optional[str]) -> str:
    """Append the detail of any linked issues to a message.

    A per-request token wins over the configured service token, so a user's own
    permissions apply when they supply one.
    """
    effective_token = token or config.JIRA_SERVICE_TOKEN
    if not effective_token or not config.JIRA_SITE_URL:
        return message

    keys = find_jira_issue_keys(message)
    if not keys:
        return message

    logger.info("enriching message with %d Jira issue(s)", len(keys))
    blocks = []
    for key in keys:
        data = fetch_issue(key, effective_token)
        if data:
            blocks.append(_render_issue_block(key, data))
        else:
            logger.info("skipping Jira issue %s: nothing retrieved", key)

    return append_context_section(message, blocks, "Jira Issue Details")
