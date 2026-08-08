"""Liveness and a summary of what the service can currently see.
"""
from __future__ import annotations

import logging

from fastapi import APIRouter

from .. import config
from ..corpus import ensure_corpus_registry

logger = logging.getLogger("praxevia.routers.health")

router = APIRouter()

@router.get("/api/health")
async def health():
    registry = ensure_corpus_registry()
    projects = {}
    for key, info in registry.items():
        projects[key] = {
            "project": info.get("project", key),
            "stream": info.get("stream", ""),
            "indexed": info["indexed"],
            "chunk_count": info["chunk_count"],
        }
    return {
        "status": "ok",
        "provider": "azure-foundry" if config.USE_AZURE_AI else "anthropic-direct",
        "projects": projects,
        "confluence_configured": bool(config.CONFLUENCE_TOKEN),
    }
