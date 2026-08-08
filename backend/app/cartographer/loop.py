"""The exploration itself: prompt, call, run tools, repeat, stream.

One coroutine drives the whole thing and yields Server-Sent Events as it goes,
so the dashboard shows tool calls as they happen rather than waiting for a final
answer that may be minutes away.

The loop is bounded by MAX_TOOL_ITERATIONS rather than by time. A question that
genuinely needs forty searches should get them; one that has started going in
circles should stop.
"""
from __future__ import annotations

import asyncio
import json
import logging
import time
from typing import AsyncGenerator, Dict, List

from ..config import (
    CLAUDE_MODEL,
    MAX_TOKENS,
    MAX_TOOL_ITERATIONS,
)
from ..corpus import (
    build_cross_project_tool_schema,
    build_tool_schema,
    ensure_corpus_registry,
    run_tool,
    run_tool_cross_project,
)
from ..telemetry import record_event
from .budget import (
    _approx_tokens,
    _approx_transcript_tokens,
    _await_with_pulse,
    _cap_tool_result,
    _condense_transcript,
    _token_budget_for_model,
)
from .prompts import _compose_cross_project_brief, _compose_project_brief
from .provider import _foundry_deployment_name, _make_anthropic_client

logger = logging.getLogger("praxevia.cartographer")

SUPPORTED_MODELS = {
    # Offered in the chat's model picker.
    "claude-opus-5",
    "claude-sonnet-5",
    "claude-haiku-4-5-20251001",
    # Retained, not offered: an existing deployment may still set CLAUDE_MODEL
    # to one of these, and silently rejecting it would break that install.
    "claude-sonnet-4-5-20250929",
    "claude-sonnet-4-6",
    "claude-opus-4-6-20250918",
    "claude-opus-4-6",
}


async def run_exploration(user_message: str, history: List[Dict], model: str = None, project: str = "", diagram_mode: str = "mermaid", reasoning_mode: str = "low-level", user_role: str = "general", user_identity: str = "", permitted_keys: List[str] = None) -> AsyncGenerator[Dict, None]:
    cross_project = permitted_keys is not None and len(permitted_keys) > 0

    registry = ensure_corpus_registry()

    if not cross_project:
        if not project or project not in registry:
            available = ", ".join(sorted(registry.keys()))
            yield {
                "type": "error",
                "data": {"message": f"Invalid or missing project: '{project}'. Available projects: {available}", "traceback": ""},
            }
            return

    client = _make_anthropic_client()
    use_model = model if model and model in SUPPORTED_MODELS else CLAUDE_MODEL
    use_model = _foundry_deployment_name(use_model)

    if cross_project:
        system_prompt = _compose_cross_project_brief(permitted_keys, diagram_mode, user_role)
        tool_defs = build_cross_project_tool_schema(permitted_keys)
    else:
        system_prompt = _compose_project_brief(project, diagram_mode, user_role)
        tool_defs = build_tool_schema(project)

    # Sanitize history: Claude API requires strict user/assistant alternation
    # starting with user. Filter empty messages, merge consecutive same-role,
    # and ensure valid structure.
    if history:
        logger.info("Incoming history: %d messages, roles=%s, content_lens=%s",
                     len(history),
                     [m.get("role") for m in history],
                     [len(m.get("content", "")) if isinstance(m.get("content"), str) else f"list({len(m.get('content', []))})" for m in history])
    raw = []
    for msg in history:
        content = msg.get("content", "")
        if isinstance(content, str) and not content.strip():
            logger.info("Filtering empty history msg: role=%s content=%r", msg.get("role"), content[:100] if content else content)
            continue
        raw.append({"role": msg["role"], "content": content})

    # Merge consecutive same-role messages
    merged = []
    for msg in raw:
        if merged and merged[-1]["role"] == msg["role"]:
            prev = merged[-1]["content"]
            content = msg["content"]
            if isinstance(prev, str) and isinstance(content, str):
                merged[-1]["content"] = prev + "\n\n" + content
                continue
        merged.append(msg)

    # Ensure alternation starts with user and ends with assistant
    messages = []
    for msg in merged:
        if not messages and msg["role"] != "user":
            continue  # Skip leading non-user messages
        messages.append(msg)
    # Trim trailing user message (new user_message will be appended)
    if messages and messages[-1]["role"] == "user":
        messages.pop()

    messages.append({"role": "user", "content": user_message})

    yield {"type": "message_start", "data": {"model": use_model}}

    _t_wall_start = time.monotonic()
    _t_api_total = 0.0
    _t_tool_total = 0.0
    _iteration_count = 0
    _t_first_text = None
    _total_output_tokens = 0

    response = None
    for iteration in range(MAX_TOOL_ITERATIONS):
        # Check if the client has disconnected (task cancelled by FastAPI)
        task = asyncio.current_task()
        if task and task.cancelled():
            logger.info("Client disconnected — stopping at iteration %d", iteration)
            break

        # Compact old tool turns if approaching context limit (model-aware)
        messages = _condense_transcript(messages, _token_budget_for_model(use_model))

        # Validate messages before API call — log and fix any empty content
        for idx, msg in enumerate(messages):
            content = msg.get("content")
            is_empty = (
                content is None
                or (isinstance(content, str) and not content.strip())
                or (isinstance(content, list) and len(content) == 0)
            )
            if is_empty:
                logger.warning(
                    "Empty content at messages[%d] role=%s type=%s — injecting placeholder",
                    idx, msg.get("role"), type(content).__name__,
                )
                if msg["role"] == "user":
                    msg["content"] = "(continued)"
                else:
                    msg["content"] = "(no response)"

        _t_api_start = time.monotonic()
        response = None
        try:
            async for _kind, _payload in _await_with_pulse(
                asyncio.to_thread(
                    client.messages.create,
                    model=use_model,
                    max_tokens=MAX_TOKENS,
                    # No temperature. The Claude 5 family rejects it outright —
                    # "`temperature` is deprecated for this model", HTTP 400 — so
                    # passing it makes every request fail against claude-opus-5,
                    # claude-sonnet-5 and claude-opus-4-8 alike. Only models back
                    # at Haiku 4.5 still accept it, and they cap output below the
                    # 128k this asks for. There is nothing to preserve here:
                    # `reasoning_mode` no longer has a temperature to vary.
                    system=system_prompt,
                    tools=tool_defs,
                    messages=messages,
                ),
                _t_api_start,
                CLAUDE_API_TIMEOUT_SECONDS,
            ):
                if _kind == "ping":
                    yield {"type": "ping", "data": {"phase": "model", "elapsed_s": _payload}}
                else:
                    response = _payload.result()
        except asyncio.TimeoutError:
            _t_api_total += time.monotonic() - _t_api_start
            yield {
                "type": "content_delta",
                "data": {"text": "\n\n[Response timed out after {}s. Please try again or simplify your question.]".format(CLAUDE_API_TIMEOUT_SECONDS)},
            }
            break
        except anthropic.APIError as e:
            _t_api_total += time.monotonic() - _t_api_start
            yield {
                "type": "content_delta",
                "data": {"text": "\n\n[API error: {}]".format(str(e))},
            }
            break
        _t_api_total += time.monotonic() - _t_api_start
        _iteration_count += 1

        # Log response details for debugging
        block_types = [b.type for b in response.content]
        text_len = sum(len(b.text) for b in response.content if b.type == "text")
        logger.info(
            "Iteration %d: stop_reason=%s, blocks=%s, text_len=%d, "
            "output_tokens=%d, input_tokens=%d",
            iteration, response.stop_reason, block_types, text_len,
            getattr(response.usage, "output_tokens", 0),
            getattr(response.usage, "input_tokens", 0),
        )

        # Process response content blocks
        tool_calls = []
        text_parts = []

        _total_output_tokens += getattr(response.usage, "output_tokens", 0)

        for block in response.content:
            if block.type == "text":
                if _t_first_text is None:
                    _t_first_text = time.monotonic()
                text_parts.append(block.text)
                # Stream text in chunks
                for i in range(0, len(block.text), 50):
                    yield {
                        "type": "content_delta",
                        "data": {"text": block.text[i:i + 50]},
                    }
                    await asyncio.sleep(0.01)  # Small delay for streaming effect

            elif block.type == "tool_use":
                tool_calls.append(block)
                yield {
                    "type": "tool_use",
                    "data": {
                        "tool_name": block.name,
                        "tool_input": block.input,
                        "tool_id": block.id,
                    },
                }

        # Check if response was truncated due to max_tokens
        if response.stop_reason == "max_tokens":
            yield {
                "type": "content_delta",
                "data": {"text": "\n\n[Response truncated — output exceeded token limit. Try asking a more specific question.]"},
            }
            break

        # If no tool calls, we're done
        if not tool_calls:
            break

        # Execute tool calls and build tool results
        # Add the assistant message with all content blocks
        assistant_content = []
        for block in response.content:
            if block.type == "text":
                assistant_content.append({"type": "text", "text": block.text})
            elif block.type == "tool_use":
                assistant_content.append({
                    "type": "tool_use",
                    "id": block.id,
                    "name": block.name,
                    "input": block.input,
                })
        messages.append({"role": "assistant", "content": assistant_content})

        # Execute tool calls in parallel; return_exceptions prevents one failure
        # from cancelling sibling tool calls
        _t_tool_start = time.monotonic()
        if cross_project:
            _gather = asyncio.gather(*[
                asyncio.to_thread(run_tool_cross_project, permitted_keys, tc.name, tc.input)
                for tc in tool_calls
            ], return_exceptions=True)
        else:
            _gather = asyncio.gather(*[
                asyncio.to_thread(run_tool, project, tc.name, tc.input)
                for tc in tool_calls
            ], return_exceptions=True)
        raw_results = []
        async for _kind, _payload in _await_with_pulse(
            _gather, _t_tool_start, CLAUDE_API_TIMEOUT_SECONDS
        ):
            if _kind == "ping":
                yield {"type": "ping", "data": {"phase": "tools", "elapsed_s": _payload}}
            else:
                raw_results = _payload.result()
        _t_tool_total += time.monotonic() - _t_tool_start

        # Record tool call stats (batch — one event per tool)
        stats_project = "__all__" if cross_project else project
        for tc in tool_calls:
            record_event(event_type="tool_call", project=stats_project, model=use_model, user_identity=user_identity)

        tool_results = []
        for tc, result_str in zip(tool_calls, raw_results):
            if isinstance(result_str, BaseException):
                logger.warning("Tool %s raised: %s", tc.name, result_str)
                result_str = json.dumps({"error": str(result_str), "count": 0})
            result_data = json.loads(result_str)

            yield {
                "type": "tool_result",
                "data": {
                    "tool_name": tc.name,
                    "tool_id": tc.id,
                    "count": result_data.get("count", 0),
                    "truncated": result_data.get("truncated", False),
                },
            }

            result_str = _cap_tool_result(result_str)

            tool_results.append({
                "type": "tool_result",
                "tool_use_id": tc.id,
                "content": result_str,
            })

        messages.append({"role": "user", "content": tool_results})
    else:
        # Loop exhausted all iterations without a final text response
        yield {
            "type": "content_delta",
            "data": {"text": "\n\n[Reached maximum tool iterations ({}). The answer above may be incomplete.]".format(MAX_TOOL_ITERATIONS)},
        }

    usage = {}
    if response is not None:
        usage = {
            "input_tokens": getattr(response.usage, "input_tokens", 0),
            "output_tokens": getattr(response.usage, "output_tokens", 0),
        }

    _t_wall_total = time.monotonic() - _t_wall_start
    _ttft = round(_t_first_text - _t_wall_start, 2) if _t_first_text else None
    _tpot_ms = round((_t_api_total / _total_output_tokens) * 1000, 1) if _total_output_tokens > 0 else None
    _tokens_per_s = round(_total_output_tokens / _t_api_total, 1) if _t_api_total > 0 else None

    yield {
        "type": "message_end",
        "data": {
            "usage": usage,
            "timing": {
                "end_to_end_s": round(_t_wall_total, 2),
                "api_time_s": round(_t_api_total, 2),
                "tool_time_s": round(_t_tool_total, 2),
                "ttft_s": _ttft,
                "tpot_ms": _tpot_ms,
                "tokens_per_s": _tokens_per_s,
                "iterations": _iteration_count,
            },
        },
    }
