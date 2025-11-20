#!/usr/bin/env bash

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"

# Get current version from Cargo.toml (root)
CURRENT_VERSION=$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')
if [ -z "${CURRENT_VERSION:-}" ]; then
  echo "0.1.0"
  exit 0
fi

# Get last tag (if any)
LAST_TAG=$(git describe --tags --abbrev=0 2>/dev/null || echo "")

if [ -n "$LAST_TAG" ]; then
  RANGE="${LAST_TAG}..HEAD"
else
  # No tag yet: treat whole history as range, default base version 0.0.0
  RANGE=""
  CURRENT_VERSION="0.0.0"
fi

# Decide bump: none / patch / minor / major
BUMP="none"

# Get commit messages in range
if [ -n "$RANGE" ]; then
  LOG_CMD=(git log --pretty=%B "$RANGE")
else
  LOG_CMD=(git log --pretty=%B)
fi

COMMITS=()
while IFS= read -r line; do
  COMMITS+=("$line")
done < <("${LOG_CMD[@]}")

for msg in "${COMMITS[@]}"; do
  first_line=$(printf "%s\n" "$msg" | head -n1)

  # BREAKING CHANGE in body?
  if printf "%s\n" "$msg" | grep -q "BREAKING CHANGE"; then
    BUMP="major"
    break
  fi

  # type! syntax (feat!:, fix!:, etc)
  if [[ "$first_line" =~ ^[a-zA-Z]+!\: ]]; then
    BUMP="major"
    break
  fi

  # If we already need major, no need to downgrade
  if [ "$BUMP" = "major" ]; then
    continue
  fi

  # Minor: any feat:
  if [[ "$first_line" =~ ^feat(\(.+\))?\: ]]; then
    if [ "$BUMP" != "minor" ]; then
      BUMP="minor"
    fi
    continue
  fi

  # Patch: fix, perf, refactor
  if [[ "$first_line" =~ ^(fix|perf|refactor)(\(.+\))?\: ]]; then
    if [ "$BUMP" = "none" ]; then
      BUMP="patch"
    fi
    continue
  fi
done

if [ "$BUMP" = "none" ]; then
  # No semver-worthy changes – just keep current
  echo "$CURRENT_VERSION"
  exit 0
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

case "$BUMP" in
  major)
    MAJOR=$((MAJOR + 1))
    MINOR=0
    PATCH=0
    ;;
  minor)
    MINOR=$((MINOR + 1))
    PATCH=0
    ;;
  patch)
    PATCH=$((PATCH + 1))
    ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
echo "$NEW_VERSION"
