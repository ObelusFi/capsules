#!/usr/bin/env bash
set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI (gh) not found. Install from https://cli.github.com/"
  exit 1
fi

ROOT="$(git rev-parse --show-toplevel)"

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$CURRENT_BRANCH" != "master" ]; then
  echo "Info: Not on master (current: $CURRENT_BRANCH). Skipping semantic release."
  exit 0
fi

# Compute next semantic version from commits
NEW_VERSION="$("$ROOT/scripts/next_version.sh")"

CURRENT_VERSION=$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')

if [ "$NEW_VERSION" = "$CURRENT_VERSION" ]; then
  echo "Info: No semantic bump required (version stays at $CURRENT_VERSION)."
  exit 0
fi

echo "Bumping version: $CURRENT_VERSION → $NEW_VERSION"

# Update Cargo.toml
tmp=$(mktemp)
sed -E "s/version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"$NEW_VERSION\"/" "$ROOT/Cargo.toml" > "$tmp"
mv "$tmp" "$ROOT/Cargo.toml"

TAG="v$NEW_VERSION"

echo "Creating tag: $TAG"
git -C "$ROOT" add Cargo.toml
git -C "$ROOT" commit -m "chore(release): $TAG"
git -C "$ROOT" tag "$TAG"

# Generate changelog section
echo "📝 Updating CHANGELOG.md..."
CHANGELOG="$ROOT/CHANGELOG.md"

if [ ! -f "$CHANGELOG" ]; then
  echo "# Changelog" > "$CHANGELOG"
  echo "" >> "$CHANGELOG"
fi

echo "## $TAG ($(date +%Y-%m-%d))" >> "$CHANGELOG"
echo "" >> "$CHANGELOG"

# Collect commits since last tag
PREV_TAG=$(git -C "$ROOT" describe --tags --abbrev=0 "$TAG^" 2>/dev/null || echo "")

if [ -z "$PREV_TAG" ]; then
  echo "First release. Listing all commits..."
  git -C "$ROOT" log --pretty=format:"- %s" >> "$CHANGELOG"
else
  echo "Changes since $PREV_TAG..."
  git -C "$ROOT" log "$PREV_TAG"..HEAD --pretty=format:"- %s" >> "$CHANGELOG"
fi

echo "" >> "$CHANGELOG"

git -C "$ROOT" add CHANGELOG.md
git -C "$ROOT" commit --amend --no-edit

echo "Building runtimes and compiler..."
"$ROOT/scripts/build_all.sh"

echo "Creating GitHub release $TAG"
gh release create "$TAG" --notes-file "$CHANGELOG"

echo "Uploading artifacts..."
gh release upload "$TAG" "$ROOT/builds/"* --clobber

echo "Pushing commit and tags..."
git -C "$ROOT" push
git -C "$ROOT" push --tags

echo "🎉 Semantic release $TAG done."
