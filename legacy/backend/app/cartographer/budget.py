"""Keeping a long exploration inside the model's context window.

An exploration can run for dozens of tool calls, and tool results are far larger
than the questions that produced them. Three mechanisms keep that bounded: a
per-result cap, whole-transcript compaction once the budget is exceeded, and a
heartbeat so the connection survives the pauses while all of it happens.

Token counts here are estimates from character length, not real tokenisation.
That is deliberate -- an exact count would mean shipping a tokeniser and paying
for it on every message, to make a decision that only needs to be roughly right.
"""
from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import Dict, List

from ..config import HEARTBEAT_INTERVAL_SECONDS

logger = logging.getLogger("praxevia.cartographer.budget")

# --- Context window management ---
# Opus 4.6 supports up to 1M input tokens (via anthropic-beta: context-1m-2025-08-07).
# Sonnet 4.5 supports up to 200K input tokens.
# 1 token ~ 4 chars is a rough estimate for English/code text.
# Note: Opus requests over 200K input tokens incur 2x input pricing.
APPROX_CHARS_PER_TOKEN = 4
RESERVED_TOKEN_HEADROOM = 75000  # reserve for system prompt + tools + output headroom
TOOL_RESULT_CHAR_CAP = 120000  # ~30K tokens per single tool result

# Per-model context limits (1M requires anthropic-beta: context-1m-2025-08-07)
CONTEXT_WINDOW_BY_MODEL = {
    # Claude 5 family — what the model picker offers.
    "claude-opus-5": 1000000,
    "claude-sonnet-5": 1000000,
    "claude-haiku-4-5-20251001": 200000,
    # 4.x kept so existing CLAUDE_MODEL settings keep working; not offered in
    # the UI any more.
    "claude-opus-4-6": 1000000,
    "claude-opus-4-6-20250918": 1000000,
    "claude-sonnet-4-6": 1000000,
    "claude-sonnet-4-5-20250929": 200000,
}
DEFAULT_CONTEXT_WINDOW = 200000


def _token_budget_for_model(model: str) -> int:
    """Return the message token budget for a given model."""
    limit = CONTEXT_WINDOW_BY_MODEL.get(model, DEFAULT_CONTEXT_WINDOW)
    return limit - RESERVED_TOKEN_HEADROOM


def _approx_tokens(text: str) -> int:
    """Rough token estimate: 1 token ~ 4 chars."""
    return len(text) // APPROX_CHARS_PER_TOKEN


def _approx_transcript_tokens(messages: List[Dict]) -> int:
    """Estimate total tokens in the messages list."""
    total = 0
    for msg in messages:
        content = msg.get("content", "")
        if isinstance(content, str):
            total += _approx_tokens(content)
        elif isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    # tool_use, tool_result, or text block
                    for val in block.values():
                        if isinstance(val, str):
                            total += _approx_tokens(val)
                        elif isinstance(val, dict):
                            total += _approx_tokens(json.dumps(val))
    return total


async def _await_with_pulse(coro, start_time: float, hard_timeout: float):
    """Drive a coroutine to completion while yielding heartbeats during the wait.

    Yields ``("ping", elapsed_seconds)`` tuples every ``HEARTBEAT_INTERVAL_SECONDS``
    the underlying call is still running, then a final ``("result", task)`` tuple.
    The caller reads ``task.result()`` to surface any exception raised by the call.
    Raises ``asyncio.TimeoutError`` if ``hard_timeout`` is exceeded.

    Heartbeats keep the SSE connection alive across the otherwise-silent gaps of a
    blocking model or tool call, so idle-sensitive proxies/load-balancers don't drop
    long-running agentic streams.
    """
    task = asyncio.ensure_future(coro)
    while True:
        done, _ = await asyncio.wait({task}, timeout=HEARTBEAT_INTERVAL_SECONDS)
        if task in done:
            yield ("result", task)
            return
        if time.monotonic() - start_time > hard_timeout:
            task.cancel()
            raise asyncio.TimeoutError()
        yield ("ping", round(time.monotonic() - start_time, 1))


def _cap_tool_result(result_str: str) -> str:
    """Truncate a single tool result if it exceeds the per-result cap."""
    if len(result_str) <= TOOL_RESULT_CHAR_CAP:
        return result_str

    try:
        data = json.loads(result_str)
    except (json.JSONDecodeError, TypeError):
        # Not JSON — hard truncate
        return result_str[:TOOL_RESULT_CHAR_CAP] + "\n[...TRUNCATED — result too large]"

    # For grep/index results: trim the matches list
    if "matches" in data and isinstance(data["matches"], list):
        original_count = len(data["matches"])
        # Keep trimming until under budget
        while len(json.dumps(data, ensure_ascii=False)) > TOOL_RESULT_CHAR_CAP and data["matches"]:
            data["matches"] = data["matches"][:len(data["matches"]) // 2]
        data["truncated"] = True
        data["showing"] = len(data["matches"])
        data["original_match_count"] = original_count
        data["note"] = f"Result truncated to fit context window (showing {len(data['matches'])} of {original_count} matches)"
        return json.dumps(data, ensure_ascii=False)

    # For call graph trace results: trim callers/callees lists
    for key in ("callers", "callees"):
        if key in data and isinstance(data[key], list) and len(data[key]) > 0:
            original_count = len(data[key])
            while len(json.dumps(data, ensure_ascii=False)) > TOOL_RESULT_CHAR_CAP and data[key]:
                data[key] = data[key][:len(data[key]) // 2]
            data[f"{key}_truncated"] = True
            data[f"{key}_showing"] = len(data[key])
            data[f"original_{key}_count"] = original_count
            data["note"] = f"Call graph truncated to fit context window (showing {len(data[key])} of {original_count} {key})"
            return json.dumps(data, ensure_ascii=False)

    # For file read results: trim content
    if "content" in data and isinstance(data["content"], str):
        original_len = len(data["content"])
        while len(json.dumps(data, ensure_ascii=False)) > TOOL_RESULT_CHAR_CAP and len(data["content"]) > 200:
            data["content"] = data["content"][:len(data["content"]) // 2]
        data["content"] += f"\n[...TRUNCATED — content too large, showing {len(data['content'])} of {original_len} chars]"
        return json.dumps(data, ensure_ascii=False)

    # Generic fallback
    serialized = json.dumps(data, ensure_ascii=False)
    if len(serialized) > TOOL_RESULT_CHAR_CAP:
        return serialized[:TOOL_RESULT_CHAR_CAP] + "\n[...TRUNCATED]"
    return serialized


def _condense_transcript(messages: List[Dict], budget_tokens: int) -> List[Dict]:
    """If messages exceed budget, summarize older tool-call/result turns.

    Keeps the first user message and the most recent turns intact.
    Replaces older assistant(tool_use)+user(tool_result) pairs with compact summaries.
    """
    est = _approx_transcript_tokens(messages)
    if est <= budget_tokens:
        return messages

    logger.info("Context compaction triggered: estimated %d tokens > budget %d", est, budget_tokens)

    # Find tool-call turn pairs (assistant with tool_use + user with tool_result)
    # We'll compact from oldest to newest until under budget
    # Keep first message (original user query) and last N turns untouched
    KEEP_RECENT_TURNS = 30  # keep last 30 messages untouched (needed for deep call-graph walks)

    if len(messages) <= KEEP_RECENT_TURNS + 1:
        return messages  # not enough to compact

    head = messages[:1]  # original user message
    middle = messages[1:-KEEP_RECENT_TURNS]
    tail = messages[-KEEP_RECENT_TURNS:]

    compacted_middle = []
    i = 0
    while i < len(middle):
        msg = middle[i]

        # Check if this is an assistant message with tool_use followed by user tool_result
        if (msg.get("role") == "assistant"
                and isinstance(msg.get("content"), list)
                and any(b.get("type") == "tool_use" for b in msg["content"] if isinstance(b, dict))
                and i + 1 < len(middle)
                and middle[i + 1].get("role") == "user"
                and isinstance(middle[i + 1].get("content"), list)):

            # Summarize the tool call pair
            tool_names = []
            for b in msg["content"]:
                if isinstance(b, dict) and b.get("type") == "tool_use":
                    tool_names.append(b.get("name", "?"))

            result_summaries = []
            for b in middle[i + 1]["content"]:
                if isinstance(b, dict) and b.get("type") == "tool_result":
                    try:
                        rd = json.loads(b.get("content", "{}"))
                        count = rd.get("count", rd.get("files_searched", "?"))
                        # Preserve key filenames/symbols so Claude retains spatial context
                        key_items: list[str] = []
                        if "matches" in rd and isinstance(rd["matches"], list):
                            for m in rd["matches"][:5]:
                                if isinstance(m, dict):
                                    key_items.append(m.get("file_path", m.get("filename", "")))
                                elif isinstance(m, str):
                                    # grep match string: "path:line: ..."
                                    key_items.append(m.split(":")[0])
                        elif "files" in rd and isinstance(rd["files"], list):
                            key_items = rd["files"][:5]
                        elif "definitions" in rd and isinstance(rd["definitions"], list):
                            key_items = [d.get("filename", "") for d in rd["definitions"][:5]]
                        key_items = [k for k in key_items if k]
                        summary = f"count={count}"
                        if key_items:
                            summary += f", paths=[{', '.join(key_items)}]"
                        result_summaries.append(summary)
                    except (json.JSONDecodeError, TypeError):
                        result_summaries.append("completed")

            summary_text = f"[Earlier search: {', '.join(tool_names)} -> {', '.join(result_summaries)}]"

            # Replace the pair with compact versions
            compact_assistant = {"role": "assistant", "content": [{"type": "text", "text": summary_text}]}
            compacted_middle.append(compact_assistant)
            i += 2  # skip the tool_result user message
        else:
            compacted_middle.append(msg)
            i += 1

    result = head + compacted_middle + tail
    new_est = _approx_transcript_tokens(result)
    logger.info("Context compaction result: %d tokens (was %d), messages %d -> %d",
                new_est, est, len(messages), len(result))
    return result
