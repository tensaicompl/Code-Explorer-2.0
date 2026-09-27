"""The agentic exploration loop.

  provider.py  building the model client, whichever provider is configured
  budget.py    keeping a long run inside the context window
  prompts.py   what the model is told before it starts
  loop.py      the iteration itself, streamed

`run_exploration` is the entry point. The private names re-exported below are
imported directly by backend/app/graph/business_flow_llm.py, which reuses the
client construction and token accounting rather than duplicating them -- moving
one of them without updating this list breaks the graph API's LLM enrichment,
and it fails at request time rather than at import.
"""
from .budget import (
    _approx_tokens,
    _approx_transcript_tokens,
    _token_budget_for_model,
)
from .loop import run_exploration
from .prompts import _compose_cross_project_brief, _compose_project_brief
from .provider import _foundry_deployment_name, _make_anthropic_client

__all__ = [
    "_approx_tokens",
    "_approx_transcript_tokens",
    "_compose_cross_project_brief",
    "_compose_project_brief",
    "_foundry_deployment_name",
    "_make_anthropic_client",
    "_token_budget_for_model",
    "run_exploration",
]
