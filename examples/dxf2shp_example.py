"""Example: DXF landscape layers → Cosia-weighted shapefile.

Requires a local DXF path (ENON-style layers). Set PYMDURS_DXF or pass as argv.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from pymdurs.geometric import dxf_to_cosia_and_weighted_layers, dxf_to_polygon_shp


def main() -> None:
    input_dxf = Path(sys.argv[1] if len(sys.argv) > 1 else os.environ.get("PYMDURS_DXF", ""))
    if not input_dxf or not input_dxf.exists():
        print("Usage: python dxf2shp_example.py <path/to/file.dxf>")
        print("Or set PYMDURS_DXF to a DXF path.")
        sys.exit(1)

    output_dir = Path(os.environ.get("PYMDURS_OUTPUT", "./output"))
    output_dir.mkdir(parents=True, exist_ok=True)
    shp_path = output_dir / "dxf_polygons.shp"
    cosia_path = output_dir / "dxf_cosia_weighted.shp"

    print(f"Converting {input_dxf} → {shp_path}")
    gdf = dxf_to_polygon_shp(input_dxf, shp_path)
    print(f"  {len(gdf)} features")

    print(f"Weighted Cosia layers → {cosia_path}")
    cosia_gdf = dxf_to_cosia_and_weighted_layers(shp_path, cosia_path)
    print(f"  {len(cosia_gdf)} features, classes: {sorted(cosia_gdf['classe'].unique())}")


if __name__ == "__main__":
    main()
