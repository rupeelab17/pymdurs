#!/usr/bin/env bash
# Incrémente la version (patch/minor/major), met à jour les fichiers, optionnellement commit + tag.
# Usage:
#   ./scripts/bump-version.sh patch              # 0.1.1 -> 0.1.2
#   ./scripts/bump-version.sh minor               # 0.1.1 -> 0.2.0
#   ./scripts/bump-version.sh major               # 0.1.1 -> 1.0.0
#   ./scripts/bump-version.sh patch --tag         # bump + commit + tag py-X.Y.Z + push
#   ./scripts/bump-version.sh patch --dev         # 0.1.1 -> 0.1.2.dev0 (pour dev après release)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BUMP="${1:-patch}"
DO_TAG=""
DO_DEV=""
for arg in "$@"; do
  [[ "$arg" == "--tag" ]] && DO_TAG=1
  [[ "$arg" == "--dev" ]] && DO_DEV=1
done

CURRENT=$(grep -m 1 -oE 'version = "[^"]+' pyproject.toml | cut -d'"' -f2)
# Enlever suffixe .dev0 / -alpha etc pour le calcul
BASE="${CURRENT%%.dev*}"
BASE="${BASE%%-*}"

read -r MAJOR MINOR PATCH <<< "$(echo "$BASE" | tr '.' ' ')"
case "$BUMP" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  patch) PATCH=$((PATCH + 1)) ;;
  *) echo "Usage: $0 patch|minor|major [--tag] [--dev]" >&2; exit 1 ;;
esac

NEW_VERSION="$MAJOR.$MINOR.$PATCH"
[[ -n "$DO_DEV" ]] && NEW_VERSION="${NEW_VERSION}.dev0"

"$ROOT/scripts/set-version.sh" "$NEW_VERSION"

if [[ -n "$DO_TAG" ]]; then
  git add pyproject.toml pymdurs/Cargo.toml rsmdu/Cargo.toml
  git commit -m "chore: release $NEW_VERSION"
  git tag "py-$NEW_VERSION"
  echo "Commit + tag py-$NEW_VERSION créés. Pousser avec: git push && git push origin py-$NEW_VERSION"
fi

echo "Version: $CURRENT -> $NEW_VERSION"
