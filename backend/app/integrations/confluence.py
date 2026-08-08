"""Pulls Confluence page content into a chat message.

When somebody pastes a link to a page and the request carries a Confluence
token, the page is fetched and its text appended to the message before the model
ever sees it -- so the model can answer about the page rather than about the URL.

Read-only: the only request this module ever issues is a GET.

Which site counts as "ours" comes from CONFLUENCE_SITE_URL. With that unset the
link pattern matches nothing and enrichment is a no-op, which is the intended
behaviour for a deployment that has no wiki to talk to.
"""
from __future__ import annotations

import base64
import logging
import re
from typing import List, Optional

import httpx

from ..config import CONFLUENCE_SITE_URL
from .rendering import append_context_section, site_link_pattern, strip_html

logger = logging.getLogger("praxevia.integrations.confluence")

#: Links into the configured wiki. None when no site is configured.
CONFLUENCE_LINK_RE = site_link_pattern(CONFLUENCE_SITE_URL) if CONFLUENCE_SITE_URL else None

#: A page id shows up either as a /pages/<id> path segment or as a pageId query
#: parameter, depending on which flavour of URL the user copied.
PAGE_ID_PATTERN = re.compile(r"(?:/pages/|[?&]pageId=)(\d+)")

_REQUEST_TIMEOUT_SECONDS = 10.0


def _page_ids_with_urls(text: str) -> dict[str, str]:
    """Map each referenced page id to the URL it was found in.

    The URL is kept so the rendered block can cite the link the user actually
    pasted, rather than a reconstructed one that may point at a different space.
    Insertion order is preserved, so pages appear in the order they were
    mentioned.
    """
    if CONFLUENCE_LINK_RE is None:
        return {}

    found: dict[str, str] = {}
    for link in CONFLUENCE_LINK_RE.finditer(text):
        for page_id in PAGE_ID_PATTERN.finditer(link.group(0)):
            found.setdefault(page_id.group(1), link.group(0))
    return found


def find_confluence_page_ids(text: str) -> List[str]:
    """Every distinct page id referenced in the text, in order of appearance."""
    return list(_page_ids_with_urls(text))


def _build_auth_header(token: str) -> str:
    """Choose the authentication scheme the token implies.

    Atlassian Cloud wants an "email:api_token" pair over Basic; a self-hosted
    Data Center instance wants a personal access token as a Bearer. The colon is
    what tells them apart, so no extra configuration is needed.
    """
    if ":" in token:
        return "Basic " + base64.b64encode(token.encode()).decode()
    return f"Bearer {token}"


def fetch_page(page_id: str, token: str) -> Optional[dict]:
    """Retrieve one page, or None if it cannot be read.

    Every failure returns None rather than raising. Enrichment is a bonus, and a
    page the user cannot see must not turn their question into an error.
    """
    if not CONFLUENCE_SITE_URL:
        return None

    url = (
        f"{CONFLUENCE_SITE_URL}/wiki/rest/api/content/{page_id}"
        "?expand=body.storage,body.view,version,space"
    )
    try:
        response = httpx.get(
            url,
            headers={"Authorization": _build_auth_header(token), "Accept": "application/json"},
            timeout=_REQUEST_TIMEOUT_SECONDS,
        )
    except Exception:
        logger.exception("could not reach Confluence for page %s", page_id)
        return None

    if response.status_code == 401:
        logger.warning("Confluence rejected the token while reading page %s", page_id)
        return None
    if response.status_code == 403:
        logger.warning(
            "Confluence refused page %s: the token either lacks permission or "
            "needs to be an email:token pair rather than a bearer token",
            page_id,
        )
        return None
    if response.status_code == 404:
        logger.warning("Confluence has no page %s", page_id)
        return None

    try:
        response.raise_for_status()
        return response.json()
    except Exception:
        logger.exception("unreadable Confluence response for page %s", page_id)
        return None


def _render_page_block(data: dict, source_url: str) -> str:
    """Turn a page payload into the markdown the model will read."""
    title = data.get("title", "untitled")
    space = (data.get("space") or {}).get("name", "unknown space")
    revision = (data.get("version") or {}).get("number", "unknown")

    body = data.get("body", {})
    # body.view is already rendered, so macros have been expanded; body.storage
    # is raw wiki markup and would leak macro syntax into the context window.
    markup = body.get("view", {}).get("value", "") or body.get("storage", {}).get("value", "")

    return "\n".join(
        [
            f"## Confluence Page: {title}",
            f"**Space**: {space} | **Version**: {revision}",
            f"**URL**: {source_url}",
            "",
            "**Content**:",
            strip_html(markup) if markup else "_This page has no readable body._",
        ]
    )


def enrich_with_confluence_context(message: str, token: Optional[str]) -> str:
    """Append the content of any linked pages to a message.

    Returns the message unchanged when there is no token, no configured site, no
    links, or nothing that could be fetched.
    """
    if not token or not CONFLUENCE_SITE_URL:
        return message

    referenced = _page_ids_with_urls(message)
    if not referenced:
        return message

    logger.info("enriching message with %d Confluence page(s)", len(referenced))
    blocks = []
    for page_id, source_url in referenced.items():
        data = fetch_page(page_id, token)
        if data:
            blocks.append(_render_page_block(data, source_url))
        else:
            logger.info("skipping Confluence page %s: nothing retrieved", page_id)

    return append_context_section(message, blocks, "Confluence Page Details")
