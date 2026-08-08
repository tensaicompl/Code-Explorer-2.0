"""Shared assembly for the enrichment integrations.

Both integrations do the same four things: find links to their own site in a
message, fetch what each one points at, render it as markdown, and append the
results. Only the middle two differ per provider, so the outer shape lives here
instead of being written twice.
"""
from __future__ import annotations

import re

#: Characters that terminate a URL when it is sitting in prose. Without this a
#: trailing bracket or comma from the surrounding sentence gets swallowed into
#: the match and the fetch then fails on a URL the user never wrote.
_URL_BODY = r"[^\s,;()\[\]<>]+"

_HTML_ENTITIES = {
    "&amp;": "&",
    "&lt;": "<",
    "&gt;": ">",
    "&quot;": '"',
    "&#39;": "'",
    "&nbsp;": " ",
}


def site_link_pattern(site_url: str) -> re.Pattern:
    """A pattern matching any link into the given site.

    The host is escaped rather than interpolated raw: a site URL is operator
    input, and an unescaped dot would quietly match hosts nobody intended.
    """
    return re.compile(re.escape(site_url.rstrip("/")) + _URL_BODY)


def strip_html(html: str) -> str:
    """Flatten a fragment of HTML into readable plain text.

    Deliberately crude. The output is destined for a model's context window, not
    a browser, so the goal is legible prose rather than fidelity: block-level
    tags become newlines, everything else is dropped, and the handful of
    entities that actually show up get decoded.
    """
    text = re.sub(r"<br\s*/?>", "\n", html)
    text = re.sub(r"</(p|div|tr|li|h[1-6])>", "\n", text)
    text = re.sub(r"<[^>]+>", "", text)
    for entity, char in _HTML_ENTITIES.items():
        text = text.replace(entity, char)
    return re.sub(r"\n{3,}", "\n\n", text).strip()


def append_context_section(message: str, blocks: list[str], heading: str) -> str:
    """Attach rendered blocks under a heading, or return the message untouched.

    Returning the original when there is nothing to add matters: every failure
    path in both integrations funnels here, so a tracker being unreachable
    degrades to an unenriched question rather than to an error the user has to
    read.
    """
    if not blocks:
        return message
    return f"{message}\n\n---\n**{heading}**:\n\n" + "\n\n".join(blocks)
