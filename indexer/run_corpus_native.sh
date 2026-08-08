#!/usr/bin/env bash
# Run the indexer natively on macOS for MPS GPU acceleration.
# Requires: Python 3.12+, Xcode CLI tools (for tree-sitter compilation).
# PostgreSQL must be running in Docker (port 5432 forwarded).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
VENV_DIR="$SCRIPT_DIR/.venv"

DATABASE_URL="${DATABASE_URL:-postgresql://prx:prx@localhost:5433/prx}"
CODEBASE_DIR="${CODEBASE_DIR:-$PROJECT_ROOT/codebase}"
EMBED_BATCH_SIZE="${EMBED_BATCH_SIZE:-256}"

# --- Check PostgreSQL connectivity ---
echo "Checking PostgreSQL connectivity..."
if ! python3 -c "
import socket, sys
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.settimeout(3)
try:
    s.connect(('localhost', 5433))
    s.close()
except Exception as e:
    print(f'Cannot connect to PostgreSQL on localhost:5433: {e}', file=sys.stderr)
    print('Make sure postgres is running: docker compose up -d postgres', file=sys.stderr)
    sys.exit(1)
"; then
    exit 1
fi
echo "PostgreSQL is reachable."

# --- Set up venv if needed ---
if [ ! -d "$VENV_DIR" ]; then
    echo "Creating virtual environment in $VENV_DIR ..."
    python3 -m venv "$VENV_DIR"
fi

source "$VENV_DIR/bin/activate"

# Install/upgrade dependencies
echo "Installing dependencies..."
pip install --quiet --upgrade pip
pip install --quiet -r "$SCRIPT_DIR/requirements.txt"

# Install PyTorch with MPS support (if not already installed with it)
python3 -c "import torch; assert torch.backends.mps.is_available()" 2>/dev/null || {
    echo "Installing PyTorch with MPS support..."
    pip install --quiet torch torchvision
}

# --- Run the indexer ---
echo "Starting native indexer (device auto-detect)..."
echo "  CODEBASE_DIR=$CODEBASE_DIR"
echo "  DATABASE_URL=$DATABASE_URL"
echo "  EMBED_BATCH_SIZE=$EMBED_BATCH_SIZE"
echo "  Usage: ./run_corpus_native.sh [project] [stream]"

export DATABASE_URL
export CODEBASE_DIR
export EMBED_BATCH_SIZE

cd "$SCRIPT_DIR"
python3 build_corpus.py "$@"
