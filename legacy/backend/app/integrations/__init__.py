"""Read-only enrichment from third-party systems.

Both integrations here take a user's message, notice links into a system the
operator has configured, fetch what those links point at, and append it as
context before the message reaches the model.

Everything in this package is strictly read-only, and both modules are written
so that any failure -- no token, no configured site, an unreachable host, a
permission refusal -- degrades to the original message rather than to an error.
Enrichment is an improvement to an answer, never a precondition for one.
"""
from .confluence import enrich_with_confluence_context, find_confluence_page_ids
from .jira import enrich_with_jira_context, find_jira_issue_keys

__all__ = [
    "enrich_with_confluence_context",
    "enrich_with_jira_context",
    "find_confluence_page_ids",
    "find_jira_issue_keys",
]
