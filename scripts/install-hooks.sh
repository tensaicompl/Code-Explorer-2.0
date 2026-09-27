#!/usr/bin/env bash
# Points this repository's hooks at scripts/, so the hook is version controlled
# rather than living untracked inside .git.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

mkdir -p .githooks
printf '#!/usr/bin/env bash\nexec "$(git rev-parse --show-toplevel)/scripts/pre-commit.sh"\n' > .githooks/pre-commit
chmod +x .githooks/pre-commit
git config core.hooksPath .githooks

echo "hooks installed: core.hooksPath is now .githooks"
echo "remove with: git config --unset core.hooksPath"
