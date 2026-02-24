#!/usr/bin/env bash
# Met à jour la version dans pyproject.toml, pymdurs/Cargo.toml et rsmdu/Cargo.toml.
# Usage: ./scripts/set-version.sh 0.1.2
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>" >&2
  echo "Ex:    $0 0.1.2" >&2
  exit 1
fi

# Validation basique (semver ou avec .dev)
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(\.[a-z0-9]+)?(-[a-z0-9.]+)?$ ]]; then
  echo "Version invalide (attendu: X.Y.Z ou X.Y.Z.dev0): $VERSION" >&2
  exit 1
fi

replace_version() {
  local file="$1"
  # Remplacer uniquement la première occurrence (package/project version)
  if [[ "$(uname)" == Darwin ]]; then
    sed -i '' "1,/version = \"[^\"]*\"/ s/version = \"[^\"]*\"/version = \"$VERSION\"/" "$file"
  else
    sed -i "1,/version = \"[^\"]*\"/ s/version = \"[^\"]*\"/version = \"$VERSION\"/" "$file"
  fi
}

replace_version pyproject.toml
replace_version pymdurs/Cargo.toml
replace_version rsmdu/Cargo.toml

echo "Version mise à jour à $VERSION dans:"
echo "  - pyproject.toml"
echo "  - pymdurs/Cargo.toml"
echo "  - rsmdu/Cargo.toml"
