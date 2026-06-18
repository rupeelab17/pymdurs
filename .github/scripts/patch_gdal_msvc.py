#!/usr/bin/env python3
"""Patch AlexanderWillner/gdal fork for MSVC bindgen (i32 vs u32 enums)."""

from __future__ import annotations

import os
import re
import sys


def patch(
    path: str,
    guard: str,
    transforms: list[tuple[str, str, int]],
    appends: list[str],
) -> None:
    if not os.path.exists(path):
        print(f"SKIP: {path}")
        return
    with open(path, encoding="utf-8") as f:
        content = f.read()
    if guard and guard in content:
        print(f"Already patched: {path}")
        return
    for pattern, repl, flags in transforms:
        content = re.sub(pattern, repl, content, flags=flags)
    if appends:
        content += "".join(line + "\n" for line in appends)
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    print(f"Patched: {path}")


def main() -> int:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <gdal_fork_src_dir>", file=sys.stderr)
        return 1

    src = sys.argv[1]

    patch(
        os.path.join(src, "src", "spatial_ref", "srs.rs"),
        "OGRErr::Type",
        [
            (
                r"let mut err_code:\s*u32\b",
                "let mut err_code: gdal_sys::OGRErr::Type",
                0,
            ),
            (r"&mut err_code as \*mut u32\b", "&mut err_code", 0),
            (
                r"^(\s+)(OAMS_TRADITIONAL_GIS_ORDER)(\s*=>)",
                r"\1x_tgo if x_tgo == \2 as u32\3",
                re.M,
            ),
            (
                r"^(\s+)(OAMS_AUTHORITY_COMPLIANT)(\s*=>)",
                r"\1x_ac if x_ac == \2 as u32\3",
                re.M,
            ),
            (
                r"^(\s+)(OAMS_CUSTOM)(\s*=>)",
                r"\1x_cu if x_cu == \2 as u32\3",
                re.M,
            ),
        ],
        [
            "/// MSVC compat added by CI patch",
            '#[cfg(target_env = "msvc")]',
            "impl TryFrom<i32> for AxisMappingStrategy {",
            "    type Error = <AxisMappingStrategy as TryFrom<u32>>::Error;",
            "    fn try_from(v: i32) -> Result<Self, Self::Error> { Self::try_from(v as u32) }",
            "}",
        ],
    )

    patch(
        os.path.join(src, "src", "raster", "types.rs"),
        "TryFrom<i32> for GdalDataType",
        [],
        [
            "/// MSVC compat added by CI patch",
            '#[cfg(target_env = "msvc")]',
            "impl TryFrom<i32> for GdalDataType {",
            "    type Error = <GdalDataType as TryFrom<u32>>::Error;",
            "    fn try_from(v: i32) -> Result<Self, Self::Error> { Self::try_from(v as u32) }",
            "}",
        ],
    )

    patch(
        os.path.join(src, "src", "raster", "rasterband.rs"),
        "GRIORA_NearestNeighbour as u32",
        [
            (
                r"(GDALRIOResampleAlg::\w+)(?=\s*[,}])",
                r"\1 as u32",
                0,
            ),
        ],
        [],
    )

    print("All patches done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
