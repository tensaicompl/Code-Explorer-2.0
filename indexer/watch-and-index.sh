#!/bin/sh
# Sidecar script: watches /data/shared for a refresh signal file,
# runs the indexer when triggered, then writes a result file.

SIGNAL_DIR="${PRX_SIGNAL_DIR:-/data/shared}"
SIGNAL_FILE="$SIGNAL_DIR/refresh-signal.json"
RESULT_FILE="$SIGNAL_DIR/refresh-result.json"
POLL_INTERVAL=10

# `python3 -m graph_pass.builder` and `import build_corpus` below both resolve against
# the working directory. The image sets WORKDIR /app, but this script is invoked
# by absolute path, so nothing else guarantees it — one `cd` is cheaper than a
# ModuleNotFoundError at three in the morning.
APP_DIR="${APP_DIR:-/app}"
cd "$APP_DIR" || {
    echo "[cadence-indexer] Cannot cd to $APP_DIR — refusing to start"
    exit 1
}

echo "[cadence-indexer] Watching $SIGNAL_DIR for refresh signals (poll every ${POLL_INTERVAL}s)"

while true; do
    if [ -f "$SIGNAL_FILE" ]; then
        echo "[cadence-indexer] Signal detected: $(cat "$SIGNAL_FILE")"
        ACTION=$(python3 -c "import json,sys; d=json.load(open('$SIGNAL_FILE')); print(d.get('action',''))" 2>/dev/null)
        PROJECT=$(python3 -c "import json,sys; d=json.load(open('$SIGNAL_FILE')); print(d.get('project',''))" 2>/dev/null)
        STREAM=$(python3 -c "import json,sys; d=json.load(open('$SIGNAL_FILE')); print(d.get('stream',''))" 2>/dev/null)

        rm -f "$SIGNAL_FILE"

        echo "[cadence-indexer] Running indexer (action=$ACTION project=$PROJECT stream=$STREAM)"
        START_TIME=$(date +%s)

        # Index, then rebuild the graph from what was just indexed.
        #
        # The graph build lives HERE rather than in refresh_engine.py because
        # this is the only place that knows the semantic index has finished
        # committing. graph_pass.builder reads the symbols_*/calls_* tables
        # build_corpus.py writes, so running it anywhere else needs a second signal to
        # answer "are those tables fresh yet?" — sequencing it in the one script
        # that already blocks on the indexer answers that for free.
        #
        # Only on success: building a graph from a half-written index would
        # publish a graph that looks complete and is not.
        if [ "$ACTION" = "index" ] && [ -n "$PROJECT" ] && [ -n "$STREAM" ]; then
            python3 /app/build_corpus.py "$PROJECT" "$STREAM"
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 0 ]; then
                echo "[cadence-indexer] Rebuilding graph for $PROJECT/$STREAM"
                python3 -m graph_pass.builder "$PROJECT" "$STREAM"
                EXIT_CODE=$?
            fi
        else
            python3 /app/build_corpus.py
            EXIT_CODE=$?
            if [ $EXIT_CODE -eq 0 ]; then
                echo "[cadence-indexer] Rebuilding graphs for all indexed projects"
                # Written to a file and read with a redirect, NOT piped into the
                # loop: /bin/sh here is dash, where the right-hand side of a pipe
                # runs in a subshell, so an EXIT_CODE set inside a piped loop is
                # discarded when the subshell exits and a failed build would be
                # reported as success.
                COMBOS_FILE=$(mktemp)
                python3 -c "
import build_corpus
for p, s in build_corpus.discover_project_streams():
    print(p, s)
" > "$COMBOS_FILE" 2>/dev/null
                while read -r P S; do
                    [ -n "$P" ] || continue
                    python3 -m graph_pass.builder "$P" "$S" || EXIT_CODE=1
                done < "$COMBOS_FILE"
                rm -f "$COMBOS_FILE"
            fi
        fi

        END_TIME=$(date +%s)
        DURATION=$((END_TIME - START_TIME))

        if [ $EXIT_CODE -eq 0 ]; then
            STATUS="success"
        else
            STATUS="failed"
        fi

        echo "{\"status\": \"$STATUS\", \"exit_code\": $EXIT_CODE, \"duration_seconds\": $DURATION, \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\"}" > "$RESULT_FILE"
        echo "[cadence-indexer] Indexer finished: status=$STATUS duration=${DURATION}s"
    fi

    sleep "$POLL_INTERVAL"
done
