"""
Example: Download LiDAR HD elevation models (MNT, MNS, MNH) from IGN WMS-R API.

This example demonstrates how to:
1. Create Mnt, Mns, and Mnh instances
2. Set a bounding box in an area covered by LiDAR HD
3. Download rasters from the IGN Géoplateforme via WMS-R
4. Save GeoTIFF outputs

Note: LiDAR HD coverage is limited to acquired zones in metropolitan France.
Use a bbox inside a covered area (see https://geoservices.ign.fr/lidarhd).
"""

import os
from pathlib import Path

import pymdurs


# Atlantec / La Rochelle area (EPSG:4326) — used elsewhere in this repository
BBOX_WGS84 = (-1.153414, 46.180217, -1.141098, 46.186531)
WORKING_CRS = 2154


def download_model(label: str, model_cls, output_path: Path):
    print(f"\n⏳ Downloading {label} from IGN WMS-R...")
    model = model_cls(output_path=str(output_path))
    model.set_bbox(*BBOX_WGS84)
    model.set_crs(WORKING_CRS)
    model = model.run()
    tiff_path = model.get_path_save_tiff()
    print(f"✅ {label} saved: {tiff_path}")
    if os.path.exists(tiff_path):
        size_mb = os.path.getsize(tiff_path) / (1024 * 1024)
        print(f"📊 File size: {size_mb:.2f} MB")
    return tiff_path


def main():
    print("🗻 LiDAR HD elevation models from IGN WMS-R")
    print(f"📦 Bounding box (WGS84): {BBOX_WGS84}")
    print("⚠️  Data is only available where LiDAR HD has been acquired.")

    output_path = Path("./output/lidar_hd_elevation").resolve()
    output_path.mkdir(parents=True, exist_ok=True)

    results = {
        "MNT": download_model("MNT (terrain)", pymdurs.geometric.Mnt, output_path),
        "MNS": download_model("MNS (surface)", pymdurs.geometric.Mns, output_path),
        "MNH": download_model("MNH (height)", pymdurs.geometric.Mnh, output_path),
    }

    print("\n✅ All LiDAR HD elevation rasters downloaded:")
    for label, path in results.items():
        print(f"  - {label}: {path}")

    return results


if __name__ == "__main__":
    main()
