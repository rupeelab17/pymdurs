"""
Example: Urban wind field (Röckle model) using pymdurs.thermal

This example demonstrates how to:
1. Collect DEM and DSM (from IGN Dem + Lidar, or use existing GeoTIFFs)
2. Load buildings (BuildingCollection) from IGN API
3. Run the Röckle wind field solver (WindField) to produce wind_speed.tif and wind_direction.tif
4. Use outputs for downstream UTCI / SOLWEIG pipelines (no QGIS required)

Prerequisites:
   - DEM.tif and DSM.tif in the output folder (if dimensions differ, DEM is
     resampled to the DSM grid automatically when rasterio is available).
   - BuildingCollection: run building_from_ign.py or pass an existing collection.
   - Optional: pip install rasterio (to auto-align DEM to DSM when grids differ).

Usage:
   python examples/wind_field_from_ign.py

Output:
   ./output/wind_speed.tif   - Wind speed (m/s) per pixel
   ./output/wind_direction.tif - Wind direction (° from North, clockwise)
"""

import os
import sys
from pathlib import Path

import pymdurs


def _align_dem_to_dsm(dem_path: Path, dsm_path: Path, output_path: Path) -> Path:
    """Resample DEM to DSM grid (shape + transform). Returns path to aligned DEM."""
    try:
        import numpy as np
        import rasterio
        from rasterio.warp import Resampling, reproject
    except ImportError:
        raise ValueError(
            "DEM and DSM have different dimensions. Install rasterio to auto-align: pip install rasterio"
        ) from None
    with rasterio.open(dsm_path) as dsm_src:
        dsm_shape = (dsm_src.height, dsm_src.width)
        dsm_transform = dsm_src.transform
        dsm_crs = dsm_src.crs
    with rasterio.open(dem_path) as dem_src:
        out_array = np.empty(dsm_shape, dtype=dem_src.dtypes[0])
        reproject(
            source=rasterio.band(dem_src, 1),
            destination=out_array,
            src_transform=dem_src.transform,
            src_crs=dem_src.crs,
            dst_transform=dsm_transform,
            dst_crs=dsm_crs,
            resampling=Resampling.bilinear,
        )
        nodata = dem_src.nodata
    with rasterio.open(
        output_path,
        "w",
        driver="GTiff",
        height=dsm_shape[0],
        width=dsm_shape[1],
        count=1,
        dtype=out_array.dtype,
        crs=dsm_crs,
        transform=dsm_transform,
        nodata=nodata,
    ) as out:
        out.write(out_array, 1)
    return output_path


def main():
    print("🌬️  Urban wind field (Röckle) with pymdurs.thermal")
    print("=" * 60)

    output_folder = "./output"
    output_path = Path(output_folder).absolute()
    output_path.mkdir(parents=True, exist_ok=True)
    output_folder_str = str(output_path)

    # Bounding box (La Rochelle area, WGS84)
    min_x, min_y, max_x, max_y = -1.152704, 46.181627, -1.139893, 46.18699

    dem_path = output_path / "DEM.tif"
    dsm_path = output_path / "DSM.tif"

    # -------------------------------------------------------------------------
    # Step 1: Ensure DEM and DSM exist (optional: run Dem + Lidar if missing)
    # -------------------------------------------------------------------------
    if not dem_path.exists():
        print("\n📥 DEM not found. Downloading from IGN API...")
        dem = pymdurs.geometric.Dem(output_path=output_folder_str)
        dem.set_bbox(min_x, min_y, max_x, max_y)
        dem.set_crs(2154)
        dem = dem.run()
        print(f"✅ DEM saved: {dem_path}")
    else:
        print(f"✅ Using existing DEM: {dem_path}")

    if not dsm_path.exists():
        print("\n📥 DSM not found. Generating from LiDAR (IGN WFS)...")
        try:
            lidar = pymdurs.geometric.Lidar(output_path=output_folder_str)
            lidar.set_bbox(min_x, min_y, max_x, max_y)
            lidar.set_crs(2154)
            lidar.run(file_name="DSM.tif", classification_list=[2, 6, 9])
            print(f"✅ DSM saved: {dsm_path}")
        except (ValueError, Exception) as e:
            print(f"⚠️  Could not generate DSM: {e}")
            print(
                "\n  The IGN LiDAR service may be temporarily unavailable (e.g. 502)."
            )
            print("  Options:")
            print(f"    - Place DEM.tif and DSM.tif in {output_path} and run again.")
            print(
                "    - Run: python examples/lidar_from_wfs.py (when the service is back)."
            )
            sys.exit(1)
    else:
        print(f"✅ Using existing DSM: {dsm_path}")

    # -------------------------------------------------------------------------
    # Step 2: Load buildings
    # -------------------------------------------------------------------------
    print("\n🏢 Loading buildings from IGN API...")
    buildings = pymdurs.geometric.Building(
        output_path=output_folder_str,
        defaultStoreyHeight=3.0,
    )
    buildings.set_bbox(min_x, min_y, max_x, max_y)
    buildings = buildings.run()
    print(f"✅ Loaded {len(buildings)} buildings")

    # -------------------------------------------------------------------------
    # Step 2b: Ensure DEM and DSM have the same dimensions (align if needed)
    # -------------------------------------------------------------------------
    dem_for_wind = dem_path
    try:
        import rasterio

        with rasterio.open(dem_path) as d, rasterio.open(dsm_path) as s:
            if (d.height, d.width) != (s.height, s.width):
                aligned = output_path / "DEM_aligned.tif"
                print(
                    f"\n📐 Aligning DEM to DSM grid (DEM {d.height}x{d.width} → DSM {s.height}x{s.width})..."
                )
                dem_for_wind = _align_dem_to_dsm(dem_path, dsm_path, aligned)
                print(f"✅ Aligned DEM saved: {dem_for_wind}")
    except ImportError:
        pass  # no rasterio: use DEM as-is; wind.run() may fail if dimensions differ

    # -------------------------------------------------------------------------
    # Step 3: Run wind field (Röckle)
    # -------------------------------------------------------------------------
    print("\n🌬️  Running Röckle wind field solver...")
    wind = pymdurs.thermal.WindField(output_path=output_folder_str)
    wind.set_bbox(min_x, min_y, max_x, max_y)

    config = pymdurs.thermal.WindConfig(
        wind_speed_ref=3.5,  # m/s at reference height
        wind_direction=25.0,  # degrees (from North, clockwise) — e.g. SW
        z_ref=10.0,  # reference height (m)
        resolution_m=2.0,  # used for metadata; actual resolution from DEM/DSM
    )

    speed_path, direction_path, zone_path = wind.run(
        config,
        str(dem_for_wind),
        str(dsm_path),
        buildings,
    )

    print("✅ Wind field complete!")
    print(f"📁 Wind speed:    {speed_path}")
    print(f"📁 Wind direction: {direction_path}")
    if zone_path is not None:
        print(f"📁 Röckle zones:  {zone_path}")

    if os.path.exists(speed_path):
        size_mb = os.path.getsize(speed_path) / (1024 * 1024)
        print(f"📊 wind_speed.tif size: {size_mb:.2f} MB")

    print("\n💡 Use these rasters as input for SOLWEIG/UTCI (e.g. pymdu or UMEP).")
    return wind, speed_path, direction_path


if __name__ == "__main__":
    wind, speed_path, direction_path = main()
