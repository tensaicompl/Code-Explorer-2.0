"""Choosing and building the client that talks to the model.

Two providers behind one call: the Anthropic API directly, or the same models
through Azure AI Foundry. Which one is used is settled here and nowhere else, so
the exploration loop never has to know which it got.

Foundry names some deployments differently from the public API, hence the alias
table -- a model id that works against one is rejected by the other.
"""
from __future__ import annotations

import logging

import anthropic
from anthropic import AnthropicFoundry

from ..config import (
    ANTHROPIC_API_KEY,
    AZURE_AI_BASE_URL,
    AZURE_AI_CLIENT_ID,
    AZURE_AI_CLIENT_SECRET,
    AZURE_AI_RESOURCE,
    AZURE_AI_TENANT_ID,
    CLAUDE_API_TIMEOUT_SECONDS,
    USE_AZURE_AI,
)

logger = logging.getLogger("praxevia.cartographer.provider")

FOUNDRY_DEPLOYMENT_ALIASES: dict[str, str] = {
    "claude-opus-4-6-20250918": "claude-opus-4-6",
    "claude-sonnet-4-5-20250929": "claude-sonnet-4-5",
}

_azure_token_provider = None


def _azure_bearer_provider():
    global _azure_token_provider
    if _azure_token_provider is None:
        from azure.identity import ClientSecretCredential, get_bearer_token_provider
        credential = ClientSecretCredential(
            tenant_id=AZURE_AI_TENANT_ID,
            client_id=AZURE_AI_CLIENT_ID,
            client_secret=AZURE_AI_CLIENT_SECRET,
        )
        _azure_token_provider = get_bearer_token_provider(
            credential, "https://ai.azure.com/.default"
        )
    return _azure_token_provider


def _make_anthropic_client():
    if not USE_AZURE_AI:
        return anthropic.Anthropic(
            api_key=ANTHROPIC_API_KEY,
            timeout=CLAUDE_API_TIMEOUT_SECONDS,
            default_headers={"anthropic-beta": "context-1m-2025-08-07"},
        )

    token_provider = _azure_bearer_provider()
    kwargs: dict = {
        "azure_ad_token_provider": token_provider,
        "timeout": CLAUDE_API_TIMEOUT_SECONDS,
    }
    if AZURE_AI_RESOURCE:
        kwargs["resource"] = AZURE_AI_RESOURCE
    elif AZURE_AI_BASE_URL:
        kwargs["base_url"] = AZURE_AI_BASE_URL
    else:
        raise ValueError(
            "USE_AZURE_AI=true but neither AZURE_AI_RESOURCE nor AZURE_AI_BASE_URL is set"
        )
    return AnthropicFoundry(**kwargs)


def _foundry_deployment_name(model: str) -> str:
    if USE_AZURE_AI:
        return FOUNDRY_DEPLOYMENT_ALIASES.get(model, model)
    return model
