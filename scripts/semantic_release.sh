#!/usr/bin/env bash
set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  echo "GitHub CLI (gh) not found. Install from https://cli.github.com/"
  exit 1
fi

ROOT="$(git rev-parse --show-toplevel)"
git -C "ROOT" fetch --tags

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
TEMP="$ROOT/Cargo.tmp"
touch $TEMP;
awk -v new="$NEW_VERSION" '
    !done && $0 ~ /version = "[0-9]+\.[0-9]+\.[0-9]+"/ {
        sub(/version = "[0-9]+\.[0-9]+\.[0-9]+"/, "version = \"" new "\"")
        done=1
    }
    { print }
' "$ROOT/Cargo.toml" > "$TEMP"
mv $TEMP "$ROOT/Cargo.toml"
TAG="v$NEW_VERSION"



echo "📝 Updating CHANGELOG.md..."
CHANGELOG="$ROOT/CHANGELOG.md"
TEMP="$ROOT/CHANGELOG.tmp"

if [ ! -f "$CHANGELOG" ]; then
  echo "# Changelog" > "$CHANGELOG"
  echo "" >> "$CHANGELOG"
fi

PREV_TAG=$(git tag --sort=-version:refname | head -n 1 || echo "")


{
  echo "## $TAG ($(date +%Y-%m-%d))"
  echo ""

  if [ -z "$PREV_TAG" ]; then
    echo "First release. Listing all commits..."
    git -C "$ROOT" log --pretty=format:"- %s"
  else
    echo "Changes since $PREV_TAG..."
    git -C "$ROOT" log "$PREV_TAG"..HEAD --pretty=format:"- %s"
  fi

  echo ""
} > "$TEMP"
RELEASE_MESSAGE=$(cat "$TEMP")

cat "$CHANGELOG" >> "$TEMP"
mv "$TEMP" "$CHANGELOG"


echo "Pushing commit and tags..."
git add .
git commit -m "chore(release): $TAG"
git tag "$TAG"
git -C "$ROOT" push
git -C "$ROOT" push --tags

echo "Creating GitHub release $TAG"
gh release create "$TAG" --notes "$RELEASE_MESSAGE"

echo "Building runtimes and compiler..."
"$ROOT/scripts/build_all.sh"

echo "Uploading artifacts..."
gh release upload "$TAG" "$ROOT/builds/"* --clobber

echo "🎉 Semantic release $TAG done."
