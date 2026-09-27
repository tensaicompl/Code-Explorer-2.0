"""Git hosting credentials: what they are, and whether they still work.

Secrets go through storage.crypto on the way in and on the way out, so nothing
in this module ever handles a raw token except in the moment it is passed to or
returned from those two calls.

Two read paths exist on purpose. `fetch_credential` and
`list_registered_credentials` return metadata only and are what the admin UI
sees; `fetch_credential_with_secret` decrypts and is called only by the code
that is about to authenticate to a git host. Keeping them separate means a
route cannot leak a token by returning the wrong row shape.
"""
from __future__ import annotations

import logging
from typing import Optional

import psycopg2
import psycopg2.extras

from ..db import read_cursor, run_once, transaction
from .crypto import decrypt_secret, encrypt_secret, encryption_configured

logger = logging.getLogger("praxevia.storage.credentials")

#: Columns safe to hand back to a caller -- everything except the secrets.
_PUBLIC_COLUMNS = """
    id, name, source_type, base_url, auth_type, ssl_verify,
    created_by, created_at, updated_at
"""


@run_once
def ensure_credential_schema() -> None:
    """Create the credentials table and warn if secrets will be stored in clear.

    `ssh_key_encrypted` is added separately because it arrived after the table
    did; ADD COLUMN IF NOT EXISTS upgrades an existing deployment in place.
    """
    with transaction() as cur:
        cur.execute(
            """
            CREATE TABLE IF NOT EXISTS credentials (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name VARCHAR(100) NOT NULL UNIQUE,
                source_type VARCHAR(20) NOT NULL,
                base_url TEXT NOT NULL,
                auth_type VARCHAR(20) NOT NULL DEFAULT 'bearer_token',
                token_encrypted TEXT NOT NULL,
                ssl_verify BOOLEAN NOT NULL DEFAULT TRUE,
                created_by VARCHAR(255) NOT NULL,
                created_at TIMESTAMP DEFAULT NOW(),
                updated_at TIMESTAMP DEFAULT NOW()
            )
            """
        )
        cur.execute(
            "ALTER TABLE credentials ADD COLUMN IF NOT EXISTS ssh_key_encrypted TEXT DEFAULT ''"
        )

    if not encryption_configured():
        logger.warning(
            "PRX_CREDENTIAL_KEY is unset or too short - git tokens and SSH keys "
            "will be written to the database in clear text"
        )
    logger.info("credentials table ready")


# ---------------------------------------------------------------------------
# Writes
# ---------------------------------------------------------------------------


def register_credential(
    name: str,
    source_type: str,
    base_url: str,
    token: str = "",
    ssl_verify: bool = True,
    created_by: str = "system",
    auth_type: str = "bearer_token",
    ssh_key: str = "",
) -> Optional[dict]:
    """Store a new credential.

    Returns None -- rather than raising -- when the name is already taken, so
    the route can answer 409 without inspecting driver exceptions.
    """
    try:
        with transaction(dict_rows=True) as cur:
            cur.execute(
                f"""
                INSERT INTO credentials
                    (name, source_type, base_url, auth_type, token_encrypted,
                     ssh_key_encrypted, ssl_verify, created_by)
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                RETURNING {_PUBLIC_COLUMNS}
                """,
                (
                    name,
                    source_type,
                    base_url,
                    auth_type,
                    encrypt_secret(token) if token else "",
                    encrypt_secret(ssh_key) if ssh_key else "",
                    ssl_verify,
                    created_by,
                ),
            )
            row = cur.fetchone()
    except psycopg2.errors.UniqueViolation:
        logger.info("credential name %r is already in use", name)
        return None
    return dict(row) if row else None


def modify_credential(
    credential_id: str,
    name: Optional[str] = None,
    base_url: Optional[str] = None,
    token: Optional[str] = None,
    ssl_verify: Optional[bool] = None,
    auth_type: Optional[str] = None,
    ssh_key: Optional[str] = None,
) -> bool:
    """Update whichever fields were supplied.

    None means "leave alone", which is what allows a caller to change a
    credential's name without resending its secret -- the secret is never
    returned to the client, so it could not resend it anyway.
    """
    # (column, value) pairs, built only for arguments that were actually passed.
    updates: list[tuple[str, object]] = []
    if name is not None:
        updates.append(("name", name))
    if base_url is not None:
        updates.append(("base_url", base_url))
    if token is not None:
        updates.append(("token_encrypted", encrypt_secret(token)))
    if ssl_verify is not None:
        updates.append(("ssl_verify", ssl_verify))
    if auth_type is not None:
        updates.append(("auth_type", auth_type))
    if ssh_key is not None:
        # An explicit empty string clears the stored key rather than encrypting
        # an empty value, which would decrypt to "" but not compare equal to it.
        updates.append(("ssh_key_encrypted", encrypt_secret(ssh_key) if ssh_key else ""))

    if not updates:
        return False

    assignments = ", ".join(f"{column} = %s" for column, _ in updates)
    values = [value for _, value in updates]

    with transaction() as cur:
        cur.execute(
            f"UPDATE credentials SET {assignments}, updated_at = NOW() WHERE id = %s",
            [*values, credential_id],
        )
        return cur.rowcount > 0


def remove_credential(credential_id: str) -> bool:
    """Delete a credential, unless a repository source still depends on it.

    Refusing here is friendlier than letting the foreign key fail: the sources
    would keep pointing at a credential that no longer exists and every refresh
    against them would start failing authentication instead.
    """
    try:
        with transaction() as cur:
            cur.execute(
                "SELECT COUNT(*) FROM repository_sources WHERE credential_id = %s",
                (credential_id,),
            )
            if cur.fetchone()[0] > 0:
                logger.info("credential %s is still in use by a source", credential_id)
                return False
            cur.execute("DELETE FROM credentials WHERE id = %s", (credential_id,))
            return cur.rowcount > 0
    except Exception as exc:
        logger.error("could not delete credential %s: %s", credential_id, exc)
        return False


# ---------------------------------------------------------------------------
# Reads
# ---------------------------------------------------------------------------


def fetch_credential(credential_id: str) -> Optional[dict]:
    """One credential's metadata. No secret material."""
    with read_cursor(dict_rows=True) as cur:
        cur.execute(
            f"SELECT {_PUBLIC_COLUMNS} FROM credentials WHERE id = %s",
            (credential_id,),
        )
        row = cur.fetchone()
    return dict(row) if row else None


def list_registered_credentials() -> list[dict]:
    """Every credential's metadata, by name. No secret material."""
    with read_cursor(dict_rows=True) as cur:
        cur.execute(f"SELECT {_PUBLIC_COLUMNS} FROM credentials ORDER BY name")
        return [dict(row) for row in cur.fetchall()]


def fetch_credential_with_secret(credential_id: str) -> Optional[dict]:
    """One credential including its decrypted token and SSH key.

    Only for code that is about to authenticate somewhere. The returned dict
    must not reach an HTTP response.
    """
    with read_cursor(dict_rows=True) as cur:
        cur.execute("SELECT * FROM credentials WHERE id = %s", (credential_id,))
        row = cur.fetchone()

    if not row:
        return None

    credential = dict(row)
    credential["token"] = decrypt_secret(credential.pop("token_encrypted"))
    stored_key = credential.pop("ssh_key_encrypted", "")
    credential["ssh_key"] = decrypt_secret(stored_key) if stored_key else ""
    return credential


# ---------------------------------------------------------------------------
# Connectivity check
# ---------------------------------------------------------------------------


def _bitbucket_probe(base_url: str, token: str) -> tuple[str, dict, dict]:
    return (
        f"{base_url}/rest/api/1.0/projects",
        {"Authorization": f"Bearer {token}"},
        {"limit": 1},
    )


def _github_probe(base_url: str, token: str) -> tuple[str, dict, dict]:
    # github.com's API lives on its own host; a self-hosted Enterprise instance
    # serves the same route under the configured base URL.
    url = "https://api.github.com/user" if base_url == "https://api.github.com" else f"{base_url}/user"
    return url, {"Authorization": f"token {token}"}, {}


def _gitlab_probe(base_url: str, token: str) -> tuple[str, dict, dict]:
    return f"{base_url}/api/v4/user", {"PRIVATE-TOKEN": token}, {}


def _azure_devops_probe(base_url: str, token: str) -> tuple[str, dict, dict]:
    # Azure DevOps takes a PAT as the password half of basic auth with an empty
    # username, so it has to be base64-encoded rather than sent as a bearer.
    import base64

    encoded = base64.b64encode(f":{token}".encode()).decode()
    return (
        f"{base_url}/_apis/projects",
        {"Authorization": f"Basic {encoded}"},
        {"api-version": "7.0", "$top": 1},
    )


#: Each provider's cheapest authenticated request. Every entry returns
#: (url, headers, params) so the request itself is issued in one place below.
_PROBES = {
    "bitbucket": _bitbucket_probe,
    "github": _github_probe,
    "gitlab": _gitlab_probe,
    "azure_devops": _azure_devops_probe,
}

_PROBE_TIMEOUT_SECONDS = 15


def check_credential_connectivity(credential_id: str) -> dict:
    """Try the credential against its host. Returns {"ok": bool, "message": str}.

    SSH credentials cannot be checked this way -- there is no cheap
    authenticated request to make -- so they are validated for format only, and
    the message says so rather than implying a successful connection.
    """
    import requests

    credential = fetch_credential_with_secret(credential_id)
    if not credential:
        return {"ok": False, "message": "no such credential"}

    if credential.get("auth_type", "bearer_token") == "ssh_key":
        from ..harvest.ssh import verify_ssh_private_key

        valid, problem = verify_ssh_private_key(credential.get("ssh_key", ""))
        if not valid:
            return {"ok": False, "message": f"SSH key is not usable: {problem}"}
        return {"ok": True, "message": "SSH key parses; connection not attempted"}

    source_type = credential["source_type"]
    probe = _PROBES.get(source_type)
    if probe is None:
        return {"ok": False, "message": f"no connectivity check for source type {source_type!r}"}

    url, headers, params = probe(credential["base_url"].rstrip("/"), credential["token"])
    try:
        response = requests.get(
            url,
            headers=headers,
            params=params or None,
            verify=credential["ssl_verify"],
            timeout=_PROBE_TIMEOUT_SECONDS,
        )
    except Exception as exc:
        return {"ok": False, "message": str(exc)}

    if response.status_code < 300:
        return {"ok": True, "message": f"reached {source_type} (HTTP {response.status_code})"}
    return {"ok": False, "message": f"HTTP {response.status_code}: {response.text[:200]}"}
