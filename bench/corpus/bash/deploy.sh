#!/usr/bin/env bash
# Deployment helper: functions, arrays, heredocs, case, traps and expansions.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
declare -A PORTS=([web]=8080 [api]=9090)
TARGETS=("staging" "production")

log() {
  local level="$1"; shift
  printf '%s [%s] %s\n' "$(date +%H:%M:%S)" "${level^^}" "$*" >&2
}

cleanup() {
  local code=$?
  rm -rf "${TMP_DIR:-/nonexistent}"
  exit "$code"
}
trap cleanup EXIT INT TERM

render_config() {
  local name="$1" port="${PORTS[$1]:-0}"
  cat <<EOF2
service: ${name}
port: ${port}
path: "${SCRIPT_DIR}/${name}"
EOF2
  cat <<-'LITERAL'
	not ${expanded} here
	LITERAL
}

deploy() {
  local target="$1"
  case "$target" in
    staging|dev) log info "deploying to $target" ;;
    production)
      [[ "${CONFIRM:-no}" == "yes" ]] || { log error "refusing production without CONFIRM=yes"; return 1; }
      ;;
    *) log error "unknown target: $target"; return 2 ;;
  esac
  for service in "${!PORTS[@]}"; do
    render_config "$service" > "${TMP_DIR}/${service}.yaml"
  done
  (( ${#PORTS[@]} > 0 )) && log info "rendered ${#PORTS[@]} configs"
}

TMP_DIR="$(mktemp -d)"
for t in "${TARGETS[@]}"; do
  deploy "$t" || log warn "deploy to $t failed with $?"
done
echo "${TARGETS[*]/#/target:}" | tr ' ' '\n' | sort -u | while read -r line; do
  echo "${line%%:*} -> ${line#*:}"
done
