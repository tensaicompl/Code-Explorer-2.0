"""Everything this service persists on a user's behalf.

  conversations.py  chat transcripts, expiring on a retention window
  preferences.py    per-user UI state
  credentials.py    git hosting credentials
  crypto.py         encryption at rest for the secrets credentials.py holds

Grouped together because they share a property worth keeping visible: all of it
is user data that can be deleted without breaking the application. The workspace
registry, which cannot, lives in harvest/ instead.

`conversations` is exported as a ready-made instance rather than a class -- see
conversations.py for why there should only be one.
"""
from .conversations import ConversationStore, conversations
from .credentials import (
    check_credential_connectivity,
    ensure_credential_schema,
    fetch_credential,
    fetch_credential_with_secret,
    list_registered_credentials,
    modify_credential,
    register_credential,
    remove_credential,
)
from .crypto import decrypt_secret, encrypt_secret, encryption_configured
from .preferences import ensure_preferences_schema, load_preferences, store_preferences

__all__ = [
    "ConversationStore",
    "check_credential_connectivity",
    "conversations",
    "decrypt_secret",
    "encrypt_secret",
    "encryption_configured",
    "ensure_credential_schema",
    "ensure_preferences_schema",
    "fetch_credential",
    "fetch_credential_with_secret",
    "list_registered_credentials",
    "load_preferences",
    "modify_credential",
    "register_credential",
    "remove_credential",
    "store_preferences",
]
