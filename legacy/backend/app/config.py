import os
from pathlib import Path


ANTHROPIC_API_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
CLAUDE_MODEL = os.environ.get("CLAUDE_MODEL", "claude-opus-5")
DATABASE_URL = os.environ.get("DATABASE_URL", "postgresql://prx:prx@postgres:5432/prx")
CODEBASE_DIR = Path(os.environ.get("CODEBASE_DIR", str(Path(__file__).resolve().parent.parent.parent / "codebase")))
# NO DEFAULTS, deliberately. These used to fall back to `admin` and a password
# committed in this repository — on any reachable host that is an open door with
# a publicly known key, and the operator gets no signal that it happened.
# Failing to start is the only safe behaviour: a service that will not boot gets
# fixed, a service that boots insecurely does not.
LOGIN_USERNAME = os.environ.get("LOGIN_USERNAME", "")
LOGIN_PASSWORD = os.environ.get("LOGIN_PASSWORD", "")
EMBEDDING_MODEL = os.environ.get("EMBEDDING_MODEL", "Alibaba-NLP/gte-base-en-v1.5")
AZURE_CLIENT_ID = os.environ.get("AZURE_CLIENT_ID", "")  # Entra user login JWT validation
AZURE_TENANT_ID = os.environ.get("AZURE_TENANT_ID", "")  # Entra user login JWT validation

# --- Azure AI Foundry (switchable Claude provider) ---
USE_AZURE_AI = os.environ.get("USE_AZURE_AI", "false").lower() == "true"
AZURE_AI_TENANT_ID = os.environ.get("AZURE_AI_TENANT_ID", "")
AZURE_AI_CLIENT_ID = os.environ.get("AZURE_AI_CLIENT_ID", "")
AZURE_AI_CLIENT_SECRET = os.environ.get("AZURE_AI_CLIENT_SECRET", "")
AZURE_AI_RESOURCE = os.environ.get("AZURE_AI_RESOURCE", "")
AZURE_AI_BASE_URL = os.environ.get("AZURE_AI_BASE_URL", "")
JIRA_SERVICE_TOKEN = os.environ.get("JIRA_SERVICE_TOKEN", "")
CONFLUENCE_TOKEN = os.environ.get("CONFLUENCE_TOKEN", "")

# --- Issue tracker and wiki that chat messages may be enriched from ---
# Both are empty by default and no site is compiled into the source. Until an
# operator points these at their own instances, link enrichment simply does not
# run: the integrations recognise links by matching the configured host, so with
# nothing configured there is nothing to match.
JIRA_SITE_URL = os.environ.get("JIRA_SITE_URL", "").rstrip("/")
CONFLUENCE_SITE_URL = os.environ.get("CONFLUENCE_SITE_URL", "").rstrip("/")
# Self-hosted trackers are commonly fronted by a certificate that is not in the
# container's trust store. This defaults to off to preserve how the integration
# has always behaved; turn it on wherever the instance presents a trusted chain.
JIRA_VERIFY_SSL = os.environ.get("JIRA_VERIFY_SSL", "false").lower() == "true"

# API keys for external integrations: comma-separated "key:client_name" pairs
# Example: API_KEYS=sk-abc123:ci-pipeline,sk-def456:monitoring-service
_raw_api_keys = os.environ.get("API_KEYS", "")
API_KEYS: dict[str, str] = {}
for _entry in _raw_api_keys.split(","):
    _entry = _entry.strip()
    if ":" in _entry:
        _k, _v = _entry.split(":", 1)
        if _k.strip() and _v.strip():
            API_KEYS[_k.strip()] = _v.strip()

# Access control
ADMIN_EMAILS = os.environ.get("ADMIN_EMAILS", "")
ACCESS_CONTROL_ENABLED = os.environ.get("ACCESS_CONTROL_ENABLED", "true").lower() == "true"

# AES-256-GCM key protecting stored git credentials: 64 hex characters, i.e.
# 32 bytes. Leaving it unset is allowed and means secrets are written in clear
# text; storage/crypto.py says so at startup rather than failing quietly.
# Read here and only here -- storage/crypto.py imports this value rather than
# consulting the environment a second time.
CREDENTIAL_ENCRYPTION_KEY = os.environ.get("PRX_CREDENTIAL_KEY", "")

# Shared signal directory for indexer sidecar
SIGNAL_DIR = Path(os.environ.get("PRX_SIGNAL_DIR", "/data/shared"))

MAX_TOOL_ITERATIONS = 150
MAX_RESULTS_DEFAULT = 100
SEARCH_TIMEOUT_SECONDS = 30
MAX_FILE_READ_LINES = 500
CLAUDE_API_TIMEOUT_SECONDS = 600
MAX_TOKENS = 128000
# SSE heartbeat: interval (seconds) at which a keepalive comment is emitted while
# waiting on a blocking model/tool call, so idle-sensitive proxies/load-balancers
# don't drop long-running agentic streams.
HEARTBEAT_INTERVAL_SECONDS = int(os.environ.get("PRX_HEARTBEAT_INTERVAL_SECONDS", "10"))
