"""
Example: Download and process LiDAR data from IGN WFS service using pymdurs

This example demonstrates how to:
1. Create a Lidar instance
2. Set a bounding box (resolves COPC URLs via WFS, no download)
3. List COPC URLs with list_copc_urls()
4. Process points on run() (download then DSM/DTM/CHM)
5. Save results as a multi-band GeoTIFF file
6. Export the point cloud (ROI) as a .las file
"""

from __future__ import annotations

import os
from collections.abc import Sequence

import pymdurs


def main():
    print("🛰️  Loading LiDAR data from IGN WFS service...")

    # Create Lidar instance
    # Following Python: lidar = Lidar(output_path="./", classification=6)
    lidar = pymdurs.geometric.Lidar(output_path="./output")
    # Enable only one CRS/bbox pair. Bboxes use WGS84 (west, south, east, north).
    # LA ROCHELLE
    # lidar.set_crs(2154)
    # lidar.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699)

    # REUNION
    # Match the native tile CRS: RGR92 / UTM 40S for La Réunion.
    # For mainland France, use EPSG:2154 (Lambert-93) instead.
    # lidar.set_crs(2975)
    # WGS84 bbox: resolves COPC URLs via WFS only (no LAZ download yet).
    # lidar.set_bbox(55.51, -21.285, 55.53, -21.265)

    # The IGN WFS index may return no LiDAR HD tiles for some territories.
    # MAYOTTE — RGM04 / UTM 38S (Mamoudzou)
    # lidar.set_crs(4471)
    # lidar.set_bbox(45.215, -12.790, 45.235, -12.770)

    # MARTINIQUE — RGAF09 / UTM 20N (Fort-de-France)
    # lidar.set_crs(5490)
    # lidar.set_bbox(-61.083, 14.595, -61.063, 14.615)

    # SAINT-PIERRE-ET-MIQUELON — RGSPM06 / UTM 21N (Saint-Pierre)
    # lidar.set_crs(4467)
    # lidar.set_bbox(-56.186, 46.770, -56.166, 46.790)

    # GUADELOUPE — RGAF09 / UTM 20N (Pointe-à-Pitre)
    lidar.set_crs(5490)
    lidar.set_bbox(-61.545, 16.230, -61.525, 16.250)

    print("📦 Bounding box set")
    geo = lidar.geo_core
    print(f"🗺️  CRS: {geo.epsg}")

    # List COPC URLs without downloading tiles
    copc_urls: Sequence[str] = lidar.list_copc_urls()
    print(f"📍 {len(copc_urls)} COPC file(s):")
    for i, url in enumerate(copc_urls, start=1):
        print(f"   {i}/{len(copc_urls)}: {url}")

    # Optional: Set classification filter
    # Following Python: classification_list=[3, 4, 5, 9]
    # 1 = unclassified, 2 = Ground, 3 = Low Vegetation, 4 = Medium Vegetation, 5 = High Vegetation, 9 = Water
    # run() downloads COPC/LAZ on first call, then builds rasters
    classification_list = [3, 4, 5]  # Vegetation classes
    lidar.run(file_name="CDSM.tif", classification_list=classification_list)

    classification_list = [3, 4, 5, 2, 6, 9, 10, 11]  # Ground and buildings classes

    output_path = lidar.run(
        file_name="DSM.tif", classification_list=classification_list
    )

    # Export points in ROI as .las
    try:
        las_path = lidar.save(filename="bbox.las")
        print(f"📦 Point cloud (ROI) saved to: {las_path}")
    except ValueError as e:
        print(f"⚠️  Could not export LAS (ensure set_bbox then run/save): {e}")
        las_path = None

    print("✅ LiDAR processing complete!")
    print(f"📁 GeoTIFF saved to: {output_path}")

    # Check if file exists
    if os.path.exists(output_path):
        size = os.path.getsize(output_path) / (1024 * 1024)  # MB
        print(f"📊 GeoTIFF file size: {size:.2f} MB")
        print("📊 File contains 3 bands:")
        print("   - Band 1: DSM (Digital Surface Model)")
        print("   - Band 2: DTM (Digital Terrain Model)")
        print("   - Band 3: CHM (Canopy Height Model)")

    return lidar, output_path


if __name__ == "__main__":
    lidar, output_path = main()
    print("\n✨ Example completed successfully!")
    print(f"   You can now open {output_path} in a GIS application like QGIS.")
