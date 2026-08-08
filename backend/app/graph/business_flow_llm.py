"""
LLM enrichment for business-flow graphs — business vocabulary, not structure.

WHAT THIS DOES AND DOES NOT DO. The deterministic pass
(`business_flow_synth.py`) already produces a correct SHAPE: which domains
exist, which flows start where, which steps follow which. What it cannot produce
is the business VOCABULARY — it names a domain after the layer it came from and
a flow after its entry function, because that is all the graph can prove.

This pass renames and annotates that skeleton. It never adds, removes or
reorders a node or an edge. Structure stays deterministic and reproducible; only
`name`, `summary` and `domainMeta` change. That boundary is the whole design: a
model that can restructure the graph can also quietly invent a business process
that does not exist in the code, and no reader could tell.

THE KEY IS THE ADVISOR'S KEY. `_make_anthropic_client()` is imported from
`cartographer.py` rather than reconstructed, so this pass authenticates exactly
the way the chat does — same `ANTHROPIC_API_KEY`, same `ANTHROPIC_BASE_URL` (this
deployment routes through a gateway, not api.anthropic.com), same timeout, same
headers. Nothing new to configure, and no second place for credentials to drift.

THE MODEL IS `CLAUDE_MODEL`, USED VERBATIM. Deliberately NOT filtered through
`SUPPORTED_MODELS`: that set is the chat model-picker's allow-list and does not
contain the model this deployment actually runs. Passing an "allowed" id here
would silently send a model the configured gateway may not serve — the failure
would look like an API error, not like a config mistake.

BOUNDED, because it runs unattended. One call, a hard input-token ceiling, a
bounded output, and a per-project cooldown enforced by the caller. There is no
resumability and no spend ledger anywhere in this repo to hook into, so the only
honest guarantee is that a single run cannot be large — not that a thousand runs
cannot be.
"""

from __future__ import annotations

import json
import logging
import re

from ..cartographer import (
    _make_anthropic_client,
    _token_budget_for_model,
    _approx_tokens,
    _foundry_deployment_name,
)
from ..config import ANTHROPIC_API_KEY, CLAUDE_MODEL

logger = logging.getLogger(__name__)

# No arbitrary input ceiling — the limit is the MODEL's, not a number picked
# here. Spend is controlled by the once-a-day cooldown in business_flow.py, so a
# second cap on context would only truncate evidence the one permitted run is
# entitled to use. `_token_budget_for_model` already subtracts the reserve the
# chat uses, and falls back to DEFAULT_CONTEXT_WINDOW for a model it does not
# know — which is every model on a gateway deployment.
#
# For scale: the five real indexes pack to 1.8k–17k tokens, against a budget of
# 125k for an unrecognised model. Trimming should never trigger in practice; it
# exists so a pathological repo degrades instead of erroring.
def _input_budget() -> int:
    return _token_budget_for_model(CLAUDE_MODEL)
# The reply is a naming table, not prose. The chat's MAX_TOKENS (128000) is
# wildly wrong for it.
MAX_OUTPUT_TOKENS = 8_000
# Steps are the bulk of the payload and the least useful part for naming a
# domain. They are trimmed first when the ceiling is hit.
MAX_STEPS_LISTED_PER_FLOW = 6

VALID_ENTRY_TYPES = {"http", "cli", "event", "cron", "manual"}

SYSTEM_PROMPT = """\
You name business domains and processes in a codebase.

You will be given a structural map of one repository: domains (areas of the \
codebase), the flows (processes) inside them, and the steps each flow runs. \
The names you see are mechanical — derived from folder and function names — and \
your job is to replace them with the words the people who run this business \
would use.

Rules, in order of importance:

1. NEVER invent structure. You may not add, remove, merge, split or reorder any \
domain, flow or step. Reply only about the ids you are given.
2. Ground every name in the evidence shown. If a domain's files are about \
carts and checkout, "Order Management" is a good name. If you cannot tell what \
something does, leave it out of your reply entirely — a mechanical name is far \
better than a confident wrong one.
3. Use the business's language, not the code's. "Reserve stock", not \
"reserve_stock_impl".
4. Keep names short — two to four words. Summaries are one or two sentences.
5. businessRules are constraints the code enforces that a person would care \
about ("an order cannot ship before payment clears"). Only state one if the \
evidence shows it. Prefer an empty list to a plausible guess.

Reply with a single JSON object and nothing else — no explanation, no markdown \
fence. Map each id you are confident about to its improvements:

{
  "<node id>": {
    "name": "...",
    "summary": "...",
    "entities": ["..."],          // domains only, optional
    "businessRules": ["..."],     // domains only, optional
    "crossDomainInteractions": ["..."],  // domains only, optional — describe
                                  // each CALLS INTO line shown for that domain
                                  // in business terms, e.g. "asks the Shop API
                                  // to price a cart before checkout"
    "entryType": "http|cli|event|cron|manual"   // flows only, optional
  }
}

Omit any field you are unsure of. Omit any id you cannot name confidently. An \
empty object is a valid, honest answer."""


def _pack(graph: dict, max_steps_per_flow: int = MAX_STEPS_LISTED_PER_FLOW) -> str:
    """
    Render the deterministic graph as the evidence the model names things from.

    Ids are included because the reply is keyed by them. File paths are included
    because they are the single strongest signal of what an area is FOR — a
    domain whose members all live under `api/app/routers/cart` is about carts,
    whatever the layer happened to be called.
    """
    by_id = {n["id"]: n for n in graph["nodes"]}
    flows_of: dict[str, list[str]] = {}
    steps_of: dict[str, list[str]] = {}
    calls_into: dict[str, list[tuple[str, str]]] = {}
    for e in graph["edges"]:
        if e["type"] == "contains_flow":
            flows_of.setdefault(e["source"], []).append(e["target"])
        elif e["type"] == "flow_step":
            steps_of.setdefault(e["source"], []).append(e["target"])
        elif e["type"] == "cross_domain":
            calls_into.setdefault(e["source"], []).append(
                (e["target"], e.get("description", ""))
            )

    lines: list[str] = []
    for node in graph["nodes"]:
        if node["type"] != "domain":
            continue
        lines.append(f'DOMAIN {node["id"]}')
        lines.append(f'  current name: {node["name"]}')
        # The evidence for crossDomainInteractions. Without it the model can
        # only guess which domains talk to each other, and a guessed
        # integration is exactly the kind of invention the prompt forbids.
        for target, description in calls_into.get(node["id"], []):
            target_node = by_id.get(target)
            if target_node is not None:
                lines.append(
                    f'  CALLS INTO {target_node["name"]}'
                    f'{f" ({description})" if description else ""}'
                )
        for flow_id in flows_of.get(node["id"], []):
            flow = by_id.get(flow_id)
            if flow is None:
                continue
            lines.append(f'  FLOW {flow_id}')
            lines.append(f'    current name: {flow["name"]}')
            for step_id in steps_of.get(flow_id, [])[:max_steps_per_flow]:
                step = by_id.get(step_id)
                if step is None:
                    continue
                where = f' [{step["filePath"]}]' if step.get("filePath") else ""
                lines.append(f'    step: {step["name"]}{where}')
    return "\n".join(lines)


def _pack_within_budget(graph: dict) -> str:
    """
    Pack, then trim steps until the context fits.

    Trimming steps rather than domains keeps every id the model is asked about
    present. Dropping a domain instead would mean it silently keeps its
    mechanical name with nothing saying why.
    """
    for max_steps in (MAX_STEPS_LISTED_PER_FLOW, 3, 1, 0):
        packed = _pack(graph, max_steps)
        if _approx_tokens(packed) <= _input_budget():
            return packed
        logger.info(
            "Business-flow context over budget at %d steps/flow, trimming", max_steps
        )
    return packed


def _extract_json(text: str) -> dict:
    """
    Pull the JSON object out of a reply that may be wrapped in prose or a fence.

    The prompt asks for bare JSON. Models comply most of the time, and the cost
    of the remaining cases is the whole enrichment, so this is worth the twelve
    lines rather than a strict parse and a failed job.
    """
    text = text.strip()
    fence = re.search(r"```(?:json)?\s*(.+?)```", text, re.S)
    if fence:
        text = fence.group(1).strip()
    if not text.startswith("{"):
        start, end = text.find("{"), text.rfind("}")
        if start == -1 or end <= start:
            raise ValueError("no JSON object in model reply")
        text = text[start : end + 1]
    parsed = json.loads(text)
    if not isinstance(parsed, dict):
        raise ValueError("model reply was not a JSON object")
    return parsed


def _merge(graph: dict, improvements: dict) -> tuple[dict, int]:
    """
    Apply the model's naming onto the deterministic graph, defensively.

    Every rule here exists because the alternative is a graph that fails
    validation or lies:

    * unknown ids are ignored — the model may hallucinate one, and adding it
      would create a node with no edges;
    * missing ids keep their deterministic values — silence means "no opinion",
      not "blank it";
    * non-string names and summaries are dropped, because `GraphNodeSchema`
      requires both to be strings and a node failing that is removed wholesale;
    * `entryType` is checked against the enum in `DomainMetaSchema`, which is a
      closed `z.enum` — an invented value fails validation for the whole node.
    """
    by_id = {n["id"]: n for n in graph["nodes"]}
    applied = 0

    for node_id, patch in improvements.items():
        node = by_id.get(node_id)
        if node is None or not isinstance(patch, dict):
            continue

        name = patch.get("name")
        if isinstance(name, str) and name.strip():
            node["name"] = name.strip()[:120]
            applied += 1

        summary = patch.get("summary")
        if isinstance(summary, str) and summary.strip():
            node["summary"] = summary.strip()[:600]

        meta = dict(node.get("domainMeta") or {})
        if node["type"] == "domain":
            for field in ("entities", "businessRules", "crossDomainInteractions"):
                value = patch.get(field)
                if isinstance(value, list):
                    meta[field] = [str(v)[:200] for v in value if isinstance(v, (str, int, float))][:12]
        if node["type"] == "flow":
            entry_type = patch.get("entryType")
            if isinstance(entry_type, str) and entry_type in VALID_ENTRY_TYPES:
                meta["entryType"] = entry_type
        if meta:
            node["domainMeta"] = meta

    return graph, applied


def enrich_business_flow_with_llm(graph: dict) -> tuple[dict, int]:
    """
    Rename and annotate a deterministic business-flow graph using the model.

    Returns `(graph, nodes_renamed)`. Raises on a missing key or an API failure —
    the caller records that on the job row. Failing loudly is deliberate: a user
    who asked for LLM naming and silently received the mechanical names would
    have no way to tell, and would judge this mode by the other one's output.
    """
    if not ANTHROPIC_API_KEY:
        raise RuntimeError(
            "ANTHROPIC_API_KEY is not set — LLM mode needs the same key the advisor uses."
        )

    packed = _pack_within_budget(graph)
    if not packed.strip():
        return graph, 0

    client = _make_anthropic_client()
    # CLAUDE_MODEL verbatim — see the module docstring on why SUPPORTED_MODELS is
    # deliberately not consulted here. It still goes through the Foundry
    # deployment map, exactly as the chat does: under USE_AZURE_AI the model id
    # and the deployment name differ, and sending the raw id there fails with an
    # API error that looks nothing like the configuration mistake it is.
    response = client.messages.create(
        model=_foundry_deployment_name(CLAUDE_MODEL),
        max_tokens=MAX_OUTPUT_TOKENS,
        system=SYSTEM_PROMPT,
        messages=[{"role": "user", "content": packed}],
    )

    # A reply cut off at the token ceiling is truncated JSON, which has no safe
    # partial form — say so rather than letting json.loads raise something that
    # reads like the model returned nonsense.
    if getattr(response, "stop_reason", None) == "max_tokens":
        raise RuntimeError(
            f"The model's reply hit the {MAX_OUTPUT_TOKENS}-token ceiling and was "
            f"truncated. Nothing was applied."
        )

    text = "".join(
        block.text for block in response.content if getattr(block, "type", "") == "text"
    )
    improvements = _extract_json(text)
    graph, applied = _merge(graph, improvements)
    logger.info(
        "LLM enrichment renamed %d/%d nodes (model=%s)",
        applied, len(graph["nodes"]), CLAUDE_MODEL,
    )
    return graph, applied
