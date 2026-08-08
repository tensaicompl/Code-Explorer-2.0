"""
Deterministic layer assignment — NOT optional polish.

A graph with zero layers makes GraphView mount the full ReactFlow tree with an
empty node set: dot grid, working minimap, controls and breadcrumb, NO nodes, and
NO message explaining why. It looks like a working tool that found nothing.
An ELK layout rejection produces the identical symptom.

RULE
    layer:<top-level-dir>   one per directory directly under codebase/{project}/{stream}/
    layer:all               fallback when the tree is flat or yields < 2 groups
    layer:other             tail grouping when the count would exceed MAX_LAYERS
    layer:root              files sitting at the tree root

This matches the index's own `caller_subsystem` grouping, so layers and
find_module_dependencies agree by construction.

ASSERTIONS — all three must FAIL THE BUILD LOUDLY, see assert_renderable().

Measured reference: awp -> 4 layers (src, tests, agent, alembic).
Layer NAMES are directory names in Tier 1; Tier 3 replaces them with authored
names and descriptions.
"""

from __future__ import annotations

# Overview lays out EVERY visible layer in ONE ELK call with no lazy path
#. More than this hangs the browser, so the tail is grouped instead.
MAX_LAYERS = 60


class LayerAssertionError(Exception):
    """Raised when the layer set would render a blank canvas. Fails the build."""


def top_level_dir(rel_path: str) -> str:
    """First path segment, or '' for a file at the tree root."""
    rel_path = rel_path.lstrip("/")
    return rel_path.split("/", 1)[0] if "/" in rel_path else ""


def assign_layers(node_ids_by_path: dict[str, list[str]]) -> list[dict]:
    """
    Map {rel_file_path: [node_id, ...]} to a list of Layer objects.

    Every node belonging to a file lands in that file's layer. Callers pass module
    node ids under a representative path so modules land in their own layer too.
    """
    groups: dict[str, list[str]] = {}
    for path, ids in node_ids_by_path.items():
        groups.setdefault(top_level_dir(path), []).extend(ids)

    # A flat tree (everything at the root) yields a single '' group. One group is
    # not a useful layering either — collapse both cases to layer:all.
    meaningful = {k: v for k, v in groups.items() if k}
    if len(meaningful) < 2:
        all_ids = [i for ids in groups.values() for i in ids]
        return _finalise([_layer("all", "All code", all_ids)])

    # Largest first, so the tail that gets grouped is the least significant.
    ordered = sorted(meaningful.items(), key=lambda kv: (-len(kv[1]), kv[0]))

    if len(ordered) > MAX_LAYERS:
        head, tail = ordered[: MAX_LAYERS - 1], ordered[MAX_LAYERS - 1 :]
        layers = [_layer(name, name, ids) for name, ids in head]
        tail_ids = [i for _, ids in tail for i in ids]
        layers.append(_layer("other", f"Other ({len(tail)} directories)", tail_ids))
    else:
        layers = [_layer(name, name, ids) for name, ids in ordered]

    # Files sitting at the tree root have no directory; fold them in rather than
    # dropping them, or their nodes belong to no layer and vanish from the view.
    root_ids = groups.get("", [])
    if root_ids:
        if layers and layers[-1]["id"] == "layer:other":
            layers[-1]["nodeIds"].extend(root_ids)
        else:
            layers.append(_layer("root", "Repository root", root_ids))

    return _finalise(layers)


def _layer(slug: str, name: str, node_ids: list[str]) -> dict:
    # De-duplicate while preserving order — a node may be reachable twice.
    seen: set[str] = set()
    unique = [i for i in node_ids if not (i in seen or seen.add(i))]
    return {
        "id": f"layer:{slug}",
        "name": name,
        "description": "",  # Tier 3 authors this
        "nodeIds": unique,
    }


def _finalise(layers: list[dict]) -> list[dict]:
    layers = [l for l in layers if l["nodeIds"]]
    assert_renderable(layers)
    return layers


def assert_renderable(layers: list[dict]) -> None:
    """
    The three assertions. Each must fail the build loudly — a violation is
    not a degraded graph, it is a blank screen that reports success.
    """
    if len(layers) < 1:
        raise LayerAssertionError(
            "Graph has zero layers. GraphView would render an interactive but "
            "completely empty canvas with no error message."
        )
    if len(layers) > MAX_LAYERS:
        raise LayerAssertionError(
            f"Graph has {len(layers)} layers, over the {MAX_LAYERS} cap. Overview "
            "lays out every visible layer in one ELK call with no lazy path "
            "; this would hang the browser."
        )
    empty = [l["id"] for l in layers if not l["nodeIds"]]
    if empty:
        raise LayerAssertionError(
            f"Layers with no nodes: {empty}. A layer with zero visible nodes is "
            "dropped from visibleLayers (GraphView.tsx:283-298), so an all-empty "
            "set reproduces the blank canvas."
        )
