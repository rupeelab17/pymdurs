#!/usr/bin/env bash
# Regenerate Python stubs from PyO3 bindings and place them for maturin / IDE resolution.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PYO3_PYTHON="${PYO3_PYTHON:-$ROOT/.venv/bin/python}"

cargo run --manifest-path pymdurs/Cargo.toml --bin stub_gen --no-default-features

GEN_DIR="$ROOT/pymdurs/pymdurs"
if [[ ! -f "$GEN_DIR/__init__.pyi" ]]; then
  echo "error: expected generated stub at $GEN_DIR/__init__.pyi" >&2
  exit 1
fi

mv "$GEN_DIR/__init__.pyi" "$ROOT/pymdurs/pymdurs.pyi"
mkdir -p "$ROOT/pymdurs/geometric" "$ROOT/pymdurs/thermal"
mv "$GEN_DIR/geometric/__init__.pyi" "$ROOT/pymdurs/geometric/__init__.pyi"
mv "$GEN_DIR/thermal/__init__.pyi" "$ROOT/pymdurs/thermal/__init__.pyi"
rm -rf "$GEN_DIR"

# Prefer public API names in cross-module references.
sed -i '' \
  -e 's/pymdurs\.PyGeoCore/pymdurs.GeoCore/g' \
  -e 's/pymdurs\.PyBoundingBox/pymdurs.BoundingBox/g' \
  -e 's/geometric\.PyBuilding/geometric.Building/g' \
  "$ROOT/pymdurs/geometric/__init__.pyi" \
  "$ROOT/pymdurs/thermal/__init__.pyi"

# pyo3-stub-gen emits Self without importing it.
for stub in "$ROOT/pymdurs/geometric/__init__.pyi" "$ROOT/pymdurs/thermal/__init__.pyi"; do
  if ! grep -q 'from typing import Self' "$stub"; then
    sed -i '' 's/from typing import TypeAlias/from typing import Self, TypeAlias/' "$stub"
  fi
done

echo "Stubs written to pymdurs/pymdurs.pyi, pymdurs/geometric/, pymdurs/thermal/"
