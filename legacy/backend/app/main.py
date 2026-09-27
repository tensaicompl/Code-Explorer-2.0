"""The ASGI application.

Assembly only: build the app, prepare the database, attach the routers, mount
the graph API. Every request handler lives in routers/; nothing here should grow
a route of its own.

The module and the `app` name are fixed by the launch contract -- the container
entrypoint and the end-to-end harness both start `app.main:app` as a literal
string, which no import checker or linter will notice going stale.
"""
from __future__ import annotations

import logging

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from . import config
from .cadence import start_cadence
from .corpus import refresh_corpus_registry
from .harvest import ensure_registry_schema
from .routers import ATTACH_IN_ORDER
from .routers.guard import ensure_project_access, identify_caller
from .storage import (
    conversations,
    ensure_credential_schema,
    ensure_preferences_schema,
)
from .telemetry import ensure_events_schema, ensure_user_projects_schema
from .warden import ensure_api_token_schema, ensure_authorization_tables

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("praxevia")

app = FastAPI(title="Praxevia Explorer API")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

for _module in ATTACH_IN_ORDER:
    app.include_router(_module.router)


@app.on_event("startup")
async def _startup() -> None:
    """Refuse to serve unconfigured, then bring the schema up and start the timer.

    The credential check comes first and raises rather than warning. These
    settings have no defaults (see config.py) precisely so that a deployment
    which forgot them fails to start: a service that will not boot gets fixed,
    whereas one that boots with a credential published in its own repository
    does not.

    Each ensure_* call is idempotent and self-limiting, so this is safe to run
    in every worker.
    """
    if not config.LOGIN_USERNAME or not config.LOGIN_PASSWORD:
        raise RuntimeError(
            "LOGIN_USERNAME and LOGIN_PASSWORD must both be set - refusing to "
            "start with no sign-in configured. Copy .env.example to .env and "
            "fill them in. Note this is a single shared development credential, "
            "not an identity system; production identity (Entra ID / MSAL) is "
            "not yet ported, so do not expose this service to an untrusted "
            "network."
        )

    ensure_events_schema()
    ensure_user_projects_schema()
    conversations.ensure_schema()
    ensure_api_token_schema()
    ensure_authorization_tables()
    ensure_preferences_schema()
    ensure_credential_schema()
    ensure_registry_schema()
    refresh_corpus_registry()
    start_cadence()


# ---------------------------------------------------------------------------
# Graph API
#
# Attached last, and handed the same two auth helpers every other route uses
# rather than being allowed its own. Somebody who cannot chat about a project
# must not be able to read its graph, and running literally the same check is
# the only way to guarantee that stays true.
#
# The whole block is guarded: the graph endpoints are additive, so a graph
# package that is broken or absent must degrade to "no graph endpoints" and
# leave the rest of the API serving.
# ---------------------------------------------------------------------------
try:
    from .graph.router import mount as _mount_graph_api
    from .graph.sources import mount as _mount_sources
    from .graph.business_flow import mount as _mount_business_flow

    _mount_graph_api(app, identify_caller, ensure_project_access)
    _mount_sources(app, identify_caller, ensure_project_access)
    _mount_business_flow(app, identify_caller, ensure_project_access)

    logger.info("graph API mounted under /api/graph")
except Exception:
    logger.exception("graph API not mounted; the rest of the API is unaffected")
