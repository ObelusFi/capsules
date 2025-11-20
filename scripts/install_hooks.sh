#!/usr/bin/env bash
set -euo pipefail

#!/usr/bin/env bash
set -e

echo "Installing git hooks..."

HOOKS_DIR="$(git rev-parse --show-toplevel)/scripts/hooks"
GIT_HOOKS_DIR="$(git rev-parse --show-toplevel)/.git/hooks"

for hook in "$HOOKS_DIR"/*; do
    name=$(basename "$hook")
    echo "Installing hook: $name"
    cp "$hook" "$GIT_HOOKS_DIR/$name"
    chmod +x "$GIT_HOOKS_DIR/$name"
done

echo "✅ Hooks installed successfully."

