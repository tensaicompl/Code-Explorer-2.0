"""YAML/JSON/TOML/env/compose -> config, service and resource nodes —

READ's parser table before estimating. What those 12 parsers actually emit
is narrower than insight.md implies:

  FREE (port the regexes + write the mapping):
    env       definitions kind="variable"
    terraform resources (dynamic kind: aws_instance, data.<type>, module)
              + definitions kind="variable"|"output"
    graphql   definitions (type/input/enum/interface/union/scalar) + endpoints
    protobuf  definitions (message/enum) + endpoints (rpc)
    makefile  steps (targets)
    markdown  sections

  STRUCTURE ONLY, no semantics — the mapping is yours to write:
    yaml/json/toml   `sections` with names and line ranges. Nothing typed.

  NOT THERE AT ALL:
    docker-compose services. compose files resolve to yaml-parser, which emits
    sections. Only dockerfile-parser emits `services`, and those are BUILD STAGES.
    insight.md's claim that "a docker-compose.yml becomes service: nodes" describes
    the LLM reading YAML sections, not a parser producing services.
    HTTP/REST endpoints. framework-registry.ts cannot hold route data — its schema
    has no such field — and it has no consumer in the pipeline at all.

FIX's FOUR REGISTRY BUGS FIRST — a config edit with the best yield per line:
  1. `openapi` is claimed by both yaml-parser and json-parser; registration order
     makes JSON win, so openapi.yaml gets JSON.parse'd, throws, and silently
     yields sections: [].
  2. kubernetes, github-actions and json-schema declare extensions: [] with no
     filenames — UNREACHABLE. Fixing these converts two coverage gaps into wins.
  3. `jsonc` is claimed but no such language id exists.
  4. types.ts:147's list of "known" definition kinds is stale — five of them are
     never emitted by any parser.
"""
