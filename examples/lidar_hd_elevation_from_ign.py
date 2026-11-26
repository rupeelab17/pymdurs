"""
Example: Download LiDAR HD elevation models (MNT, MNS, MNH) from IGN WMS-R API.

This example demonstrates how to:
1. Create Mnt, Mns, and Mnh instances
2. Set a bounding box in an area covered by LiDAR HD
3. Download rasters from the IGN Géoplateforme via WMS-R
4. Save GeoTIFF outputs

Note: LiDAR HD elevation coverage depends on the WMS-R layer and area.
Use a bbox inside a covered area (see https://geoservices.ign.fr/lidarhd).
"""

import os
from pathlib import Path

import pymdurs


# Enable only one bbox/CRS pair. Bboxes use WGS84 (west, south, east, north).
# LA ROCHELLE — Lambert-93 (Atlantec)
BBOX_WGS84 = (-1.153414, 46.180217, -1.141098, 46.186531)
WORKING_CRS = 2154

# LA RÉUNION — RGR92 / UTM 40S
# BBOX_WGS84 = (55.51, -21.285, 55.53, -21.265)
# WORKING_CRS = 2975

# MAYOTTE — RGM04 / UTM 38S (Mamoudzou)
# BBOX_WGS84 = (45.215, -12.790, 45.235, -12.770)
# WORKING_CRS = 4471

# MARTINIQUE — RGAF09 / UTM 20N (Fort-de-France)
# BBOX_WGS84 = (-61.083, 14.595, -61.063, 14.615)
# WORKING_CRS = 5490

# SAINT-PIERRE-ET-MIQUELON — RGSPM06 / UTM 21N (Saint-Pierre)
# BBOX_WGS84 = (-56.186, 46.770, -56.166, 46.790)
# WORKING_CRS = 4467

# GUADELOUPE — RGAF09 / UTM 20N (Pointe-à-Pitre)
# BBOX_WGS84 = (-61.545, 16.230, -61.525, 16.250)
# WORKING_CRS = 5490


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
