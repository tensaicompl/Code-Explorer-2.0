"""Request authentication and the checks every route shares.

No routes live here. `identify_caller` turns an Authorization header into an
identity string and is the first line of essentially every handler; the two
`ensure_*` helpers raise 403 directly rather than returning a verdict, so a
handler that forgets to inspect a result cannot accidentally continue.

The graph package is handed `identify_caller` and `ensure_project_access` at
mount time rather than implementing its own, so a user who cannot chat about a
project cannot read its graph either.
"""
from __future__ import annotations

import hashlib
import logging

from fastapi import APIRouter, HTTPException, Request

from .. import config
from ..warden import is_platform_admin, looks_like_api_token, resolve_api_token, user_can_access_project

logger = logging.getLogger("praxevia.routers.guard")

#: The bearer token issued by /api/login.
#:
#: Derived from the configured credentials rather than generated, so every
#: uvicorn worker computes the same value independently -- a random token would
#: mean a session minted by one worker was rejected by the next.
LOGIN_SESSION_TOKEN = hashlib.sha256(
    f"{config.LOGIN_USERNAME}:{config.LOGIN_PASSWORD}:session".encode()
).hexdigest()


def identify_caller(request: Request) -> str:
    """Validate token and return user identifier (email or '__password__')."""
    auth = request.headers.get("Authorization", "")
    if not auth.startswith("Bearer "):
        raise HTTPException(status_code=401, detail="Unauthorized")

    token = auth[7:]

    # Try session token first
    if token == LOGIN_SESSION_TOKEN:
        return "__password__"

    # Try static API key (from env)
    if token in config.API_KEYS:
        return f"__apikey__:{config.API_KEYS[token]}"

    # Then a user-issued API token. The prefix test only decides whether a
    # database round-trip is worth making; the token is identified by digest.
    if looks_like_api_token(token):
        owner = resolve_api_token(token)
        if owner:
            return owner

    # Try Entra ID JWT validation if configured
    if config.AZURE_CLIENT_ID:
        try:
            from ..warden import verify_entra_id_token
            claims = verify_entra_id_token(token)
            email = claims.get("email") or claims.get("preferred_username", "")
            if email:
                return email.lower()
        except ValueError as e:
            error_msg = str(e)
            logger.warning("Entra token validation failed: %s", error_msg)
            if "expired" in error_msg.lower():
                raise HTTPException(status_code=401, detail="Token expired")
        except Exception:
            logger.exception("Unexpected error during Entra token validation")

    raise HTTPException(status_code=401, detail="Unauthorized")


def is_full_access_identity(identity: str) -> bool:
    """__password__ and __apikey__:* have full access."""
    return identity == "__password__" or identity.startswith("__apikey__:")


def ensure_project_access(identity: str, project: str) -> None:
    """Raise 403 if no access. Privileged identities bypass."""
    if not config.ACCESS_CONTROL_ENABLED:
        return
    if is_full_access_identity(identity):
        return
    if not user_can_access_project(identity, project):
        raise HTTPException(status_code=403, detail="No access to this project")


def ensure_admin(identity: str) -> None:
    """Raise 403 if not admin. Privileged identities pass."""
    if is_full_access_identity(identity):
        return
    if not is_platform_admin(identity):
        raise HTTPException(status_code=403, detail="Admin access required")




def require_sso_identity(request: Request) -> str:
    """Verify token and require it to be an Entra user (email with @)."""
    user = identify_caller(request)
    if "@" not in user:
        raise HTTPException(
            status_code=403,
            detail="API key management requires SSO authentication",
        )
    return user


def to_json_safe(obj):
    """Convert datetime objects in a dict to ISO strings for JSON serialization."""
    if obj is None:
        return None
    if isinstance(obj, list):
        return [to_json_safe(item) for item in obj]
    if not isinstance(obj, dict):
        return obj
    result = {}
    for k, v in obj.items():
        if hasattr(v, "isoformat"):
            result[k] = v.isoformat()
        elif isinstance(v, list):
            result[k] = [to_json_safe(item) for item in v]
        elif isinstance(v, dict):
            result[k] = to_json_safe(v)
        else:
            result[k] = str(v) if isinstance(v, (type(None),)) is False and not isinstance(v, (str, int, float, bool)) else v
    return result


# ── User Preferences ─────────────────────────────────────────────────
