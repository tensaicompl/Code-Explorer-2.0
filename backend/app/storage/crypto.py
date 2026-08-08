"""Envelope encryption for stored secrets.

Git hosting tokens and SSH private keys are encrypted at rest with AES-256-GCM
before they reach the database, using a key supplied out of band as
PRX_CREDENTIAL_KEY (64 hex characters -- 32 bytes).

Encryption is optional, and that is a real trade-off rather than an oversight: a
deployment with no key configured still works, but stores secrets in clear text.
`ensure_credential_schema` logs a warning at startup when that is the case, and
`encryption_configured()` is exposed so callers can surface it in an operator UI
rather than leaving it to be discovered by reading the table.

Ciphertext is tagged with an "ENC:" marker so a stored value can always be told
apart from a plaintext one. That is what lets a deployment turn encryption on
without a migration: pre-existing plaintext rows keep decrypting to themselves,
and are re-encrypted the next time they are written.
"""
from __future__ import annotations

import base64
import logging
import secrets

from ..config import CREDENTIAL_ENCRYPTION_KEY

logger = logging.getLogger("praxevia.storage.crypto")

#: Marker prefixed to every encrypted value.
CIPHERTEXT_MARKER = "ENC:"

#: AES-GCM nonce width in bytes. Part of the stored format -- changing it makes
#: every existing ciphertext undecryptable.
_NONCE_BYTES = 12

#: A 32-byte key written as hex.
_EXPECTED_KEY_CHARS = 64

try:
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    _CRYPTO_AVAILABLE = True
except ImportError:  # pragma: no cover - depends on how the image was built
    _CRYPTO_AVAILABLE = False


def encryption_configured() -> bool:
    """Whether secrets written from now on will actually be encrypted.

    Both conditions matter: the cryptography package has to be installed, and a
    key of the right length has to be present. A short or absent key is treated
    as "not configured" rather than as an error, so a deployment that never set
    one keeps working.
    """
    return _CRYPTO_AVAILABLE and len(CREDENTIAL_ENCRYPTION_KEY) >= _EXPECTED_KEY_CHARS


def _load_aes_key() -> bytes:
    """Decode the configured key from hex into raw bytes."""
    return bytes.fromhex(CREDENTIAL_ENCRYPTION_KEY)


def encrypt_secret(plaintext: str) -> str:
    """Encrypt a secret for storage, or hand it back unchanged if no key is set.

    A fresh random nonce is generated per call and stored alongside the
    ciphertext. Reusing a nonce under one key would break GCM's guarantees
    outright, so it must never be derived from the value being encrypted.
    """
    if not encryption_configured():
        return plaintext

    nonce = secrets.token_bytes(_NONCE_BYTES)
    sealed = AESGCM(_load_aes_key()).encrypt(nonce, plaintext.encode("utf-8"), None)
    return CIPHERTEXT_MARKER + base64.b64encode(nonce + sealed).decode("ascii")


def decrypt_secret(stored: str) -> str:
    """Recover a stored secret.

    Values without the marker predate encryption being enabled and are returned
    as they are. A marked value with no key available raises: returning the
    ciphertext would hand a caller an unusable string that looks like a token,
    and it would then be presented to a git host as a password.
    """
    if not stored.startswith(CIPHERTEXT_MARKER):
        return stored

    if not encryption_configured():
        raise ValueError(
            "this secret is encrypted but PRX_CREDENTIAL_KEY is missing or too "
            "short; set the original 64-character key to read it"
        )

    raw = base64.b64decode(stored[len(CIPHERTEXT_MARKER):])
    nonce, sealed = raw[:_NONCE_BYTES], raw[_NONCE_BYTES:]
    return AESGCM(_load_aes_key()).decrypt(nonce, sealed, None).decode("utf-8")
