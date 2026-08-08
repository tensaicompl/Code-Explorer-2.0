"""SSH private keys for git operations.

Lives under harvest/ because cloning is what it exists for, even though the
credential admin screen also calls it to check a key before storing it.

The temporary-file dance in `ssh_private_key_tempfile` is unavoidable: git will
only take an identity file by path, so a key held in the database has to touch
the filesystem for the length of one clone. It is written with 0600 permissions
and removed on the way out, including when the clone raises.
"""
from __future__ import annotations

import logging
import os
import stat
import tempfile
from contextlib import contextmanager
from typing import Tuple

logger = logging.getLogger("praxevia.harvest.ssh")

#: Prefix on the short-lived key files, so anything left behind by a hard kill
#: is identifiable in the temp directory.
_TEMPFILE_PREFIX = "prx_ssh_"


def verify_ssh_private_key(key_content: str) -> Tuple[bool, str]:
    """Check a private key is usable for unattended cloning.

    Returns (ok, reason). The reason is shown to whoever is adding the
    credential, so it says what to do rather than just what failed.

    Passphrase-protected keys are rejected on purpose: nothing is available to
    type the passphrase during a scheduled refresh, so accepting one would store
    a credential that could only ever fail, hours later, in a background job.
    """
    if not key_content or not key_content.strip():
        return False, "no key was provided"

    key_content = key_content.strip()
    if not key_content.startswith("-----BEGIN"):
        return False, "expected a PEM block beginning with -----BEGIN ... PRIVATE KEY-----"

    try:
        from cryptography.hazmat.primitives.serialization import (
            load_pem_private_key,
            load_ssh_private_key,
        )
    except ImportError:
        # Without the cryptography package the best available check is that the
        # text at least claims to be a private key.
        if "PRIVATE KEY" not in key_content:
            return False, "this does not look like a private key"
        return True, ""

    key_bytes = key_content.encode("utf-8")
    try:
        if "OPENSSH PRIVATE KEY" in key_content:
            load_ssh_private_key(key_bytes, password=None)
        else:
            load_pem_private_key(key_bytes, password=None)
    except TypeError:
        # Raised when the loader wanted a password and got None.
        return False, "the key is passphrase-protected, which unattended cloning cannot use"
    except ValueError as exc:
        return False, f"the key could not be parsed: {exc}"
    except Exception as exc:
        return False, f"the key was rejected: {exc}"

    return True, ""


@contextmanager
def ssh_private_key_tempfile(key_content: str):
    """Materialise a key on disk for the duration of the block.

    The descriptor is closed before chmod so the mode applies to a settled file,
    and removal happens in a finally so a failed clone does not leave a private
    key sitting in the temp directory.
    """
    handle, path = tempfile.mkstemp(prefix=_TEMPFILE_PREFIX, suffix=".key")
    try:
        # ssh insists on a trailing newline and rejects the key without one.
        os.write(handle, (key_content.strip() + "\n").encode("utf-8"))
        os.close(handle)
        os.chmod(path, stat.S_IRUSR | stat.S_IWUSR)
        yield path
    finally:
        try:
            os.unlink(path)
        except OSError:
            logger.warning("could not remove temporary key file %s", path)


def git_ssh_command_for_key(key_path: str) -> str:
    """Build the GIT_SSH_COMMAND that makes git use exactly this key.

    Host key checking is disabled and known_hosts is pointed at /dev/null. That
    is a deliberate trade: these clones run unattended against hosts an operator
    has already configured, and there is no interactive session in which to
    accept a fingerprint. It does mean the transport is not protected against an
    attacker who can already redirect traffic to the git host.
    """
    return (
        f"ssh -i {key_path} "
        "-o StrictHostKeyChecking=no "
        "-o UserKnownHostsFile=/dev/null"
    )
