"""Compute Tmrt and UTCI for a 1 km × 1 km square in central Bordeaux.

Install pymdurs[thermal], then run from a checkout:

    python -m pymdurs.examples.utci_bbox --dem /path/to/ground.tif \
        --dsm /path/to/ground_and_buildings.tif --epw examples/bordeaux_2025.epw \
        --when '2025-07-01 13:00' --output /path/to/utci-output

Prepare a bare-earth DEM and an absolute-elevation DSM including building roofs
(e.g. use co-registered IGN LiDAR HD terrain and surface rasters). Both must be
in Lambert-93 (EPSG:2154), use the same vertical datum, cover the full square
and have valid elevations there. Optionally pass a canopy height model in metres
above ground and a SOLWEIG-compatible land-cover grid, also in Lambert-93.

SOLWEIG aligns the rasters on a 5 m grid, computes walls and SVF,
loads EPW weather at the selected local standard time and computes Tmrt and
UTCI GeoTIFFs. UTCI uses the EPW reference wind, not a local urban wind model.
The example does not download inputs or model shading from outside the square.
"""

from __future__ import annotations

import argparse
from pathlib import Path
from tempfile import TemporaryDirectory

import rasterio
from rasterio.crs import CRS
from rasterio.warp import transform

from pymdurs import BoundingBox

_BORDEAUX = (-0.5740, 44.8378)  # longitude, latitude in WGS84


def _bbox() -> BoundingBox:
    projected = transform("EPSG:4326", "EPSG:2154", [_BORDEAUX[0]], [_BORDEAUX[1]])
    x, y = projected[0][0], projected[1][0]
    return BoundingBox(x - 500, y - 500, x + 500, y + 500)


def main(argv: list[str] | None = None) -> None:
    """Run the local, one-timestep UTCI workflow.

    Args:
        argv: Command-line arguments, or None to use the process arguments.
    """
    parser = argparse.ArgumentParser(description="Compute Tmrt and UTCI in Bordeaux.")
    parser.add_argument("--dem", type=Path, required=True)
    parser.add_argument("--dsm", type=Path, required=True)
    parser.add_argument("--cdsm", type=Path)
    parser.add_argument("--land-cover", type=Path)
    parser.add_argument("--epw", type=Path, required=True)
    parser.add_argument(
        "--when", required=True, help="EPW local time: YYYY-MM-DD HH:MM"
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)

    bbox = _bbox()
    lambert93 = CRS.from_epsg(2154)
    for path in (args.dem, args.dsm, args.cdsm, args.land_cover):
        if path is None:
            continue
        with rasterio.open(path) as src:
            if src.crs != lambert93:
                parser.error(f"{path} must be in EPSG:2154")
            bounds = src.bounds
            if not (
                bounds.left <= bbox.min_x < bbox.max_x <= bounds.right
                and bounds.bottom <= bbox.min_y < bbox.max_y <= bounds.top
            ):
                parser.error(f"{path} does not cover the requested Bordeaux square")

    import solweig

    weather = solweig.Weather.from_epw(args.epw, start=args.when, end=args.when)
    if len(weather) != 1:
        parser.error("--when must select exactly one EPW timestep")
    station = solweig.Location.from_epw(args.epw)
    location = solweig.Location(
        latitude=_BORDEAUX[1], longitude=_BORDEAUX[0], utc_offset=station.utc_offset
    )
    # A fresh cache ensures SVF and wall rasters always match these inputs.
    with TemporaryDirectory(prefix="pymdurs-utci-") as working_dir:
        surface = solweig.SurfaceData.prepare(
            dem=args.dem,
            dsm=args.dsm,
            cdsm=args.cdsm,
            land_cover=args.land_cover,
            working_dir=working_dir,
            bbox=[bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y],
            pixel_size=5.0,
        )
        solweig.calculate(
            surface=surface,
            weather=weather,
            location=location,
            output_dir=args.output,
            outputs=["tmrt", "utci"],
        )


if __name__ == "__main__":
    main()
