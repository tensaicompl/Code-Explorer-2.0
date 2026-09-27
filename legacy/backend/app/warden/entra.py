"""Verification of Microsoft Entra ID bearer tokens.

Signature checking needs Microsoft's current public signing keys, which are
published at a well-known discovery endpoint and rotate periodically. They are
cached for a day; a token naming a key we have not seen forces one immediate
refetch before the token is rejected, which is what makes a rotation invisible
to users instead of an outage lasting until the cache expires.

Every failure path raises ValueError with a short reason. The caller turns that
into a 401 -- deliberately without echoing the detail back to the client, since
"invalid audience" tells an attacker more than "unauthorized" does.
"""
from __future__ import annotations

import logging
import time
from typing import Optional

import jwt
import requests

from .. import config

logger = logging.getLogger("praxevia.warden.entra")

#: How long a fetched key set stays usable before it is refetched.
_SIGNING_KEYS_TTL_SECONDS = 86_400

_signing_keys_cache: dict = {"keys": [], "fetched_at": 0.0}


def _discovery_url() -> str:
    """Where this tenant publishes its signing keys.

    Falling back to "common" makes the multi-tenant case work; a single-tenant
    deployment should set AZURE_TENANT_ID so tokens from other directories are
    rejected at the issuer check below.
    """
    tenant = config.AZURE_TENANT_ID or "common"
    return f"https://login.microsoftonline.com/{tenant}/discovery/v2.0/keys"


def _fetch_signing_keys() -> list[dict]:
    """Return the cached key set, refetching it when it has gone stale.

    If the refetch fails but a previous key set is still held, that one is
    returned rather than raising: expired-but-present keys will usually still
    verify current tokens, so a transient network fault should not log everybody
    out. Only a failure with nothing cached propagates.
    """
    now = time.time()
    cached = _signing_keys_cache["keys"]
    if cached and (now - _signing_keys_cache["fetched_at"]) < _SIGNING_KEYS_TTL_SECONDS:
        return cached

    url = _discovery_url()
    try:
        response = requests.get(url, timeout=10)
        response.raise_for_status()
        keys = response.json().get("keys", [])
    except Exception:
        logger.exception("could not reach the signing key endpoint at %s", url)
        if cached:
            return cached
        raise

    _signing_keys_cache["keys"] = keys
    _signing_keys_cache["fetched_at"] = now
    logger.info("cached %d signing key(s) from %s", len(keys), url)
    return keys


def _match_key(keys: list[dict], kid: str) -> Optional[object]:
    """Find the signing key a token names, as an RSA key object."""
    for entry in keys:
        if entry.get("kid") == kid:
            return jwt.algorithms.RSAAlgorithm.from_jwk(entry)
    return None


def _accepted_issuers() -> list[str]:
    """Both issuer spellings Entra uses.

    v1 and v2 tokens name the same directory two different ways, and which one
    arrives depends on how the client requested it -- accepting only one form
    rejects perfectly valid tokens.
    """
    tenant = config.AZURE_TENANT_ID or "common"
    return [
        f"https://login.microsoftonline.com/{tenant}/v2.0",
        f"https://sts.windows.net/{tenant}/",
    ]


def verify_entra_id_token(token: str) -> dict:
    """Check a token's signature and claims, returning them on success.

    Raises ValueError if the token is malformed, signed by a key we cannot
    find, expired, or issued for a different directory or application.
    """
    try:
        header = jwt.get_unverified_header(token)
    except jwt.exceptions.DecodeError as exc:
        raise ValueError(f"token header is not readable: {exc}")

    kid = header.get("kid")
    if not kid:
        raise ValueError("token header names no signing key")

    signing_key = _match_key(_fetch_signing_keys(), kid)
    if signing_key is None:
        # Most likely the directory rotated its keys since the last fetch.
        # Expire the cache and look once more before giving up.
        _signing_keys_cache["fetched_at"] = 0.0
        signing_key = _match_key(_fetch_signing_keys(), kid)

    if signing_key is None:
        raise ValueError(f"no published signing key matches kid={kid}")

    try:
        return jwt.decode(
            token,
            key=signing_key,
            algorithms=["RS256"],
            audience=config.AZURE_CLIENT_ID,
            issuer=_accepted_issuers(),
            options={"require": ["exp", "iss", "aud"]},
        )
    except jwt.ExpiredSignatureError:
        raise ValueError("token has expired")
    except jwt.InvalidIssuerError:
        raise ValueError("token came from an unexpected directory")
    except jwt.InvalidAudienceError:
        raise ValueError("token was issued for a different application")
    except jwt.PyJWTError as exc:
        raise ValueError(f"token failed verification: {exc}")
