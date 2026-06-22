"""
Example: Download DEM (Digital Elevation Model) from IGN API using rsmdu

This example demonstrates how to:
1. Create a Dem instance
2. Set a bounding box
3. Download DEM from IGN API via WMS-R
4. Get output paths for TIFF and mask files
"""

import os

import pymdurs


def main():
    print("🗻 Loading DEM from IGN API...")

    # Create Dem instance (using alias created by rsmdu_helper)
    dem = pymdurs.geometric.Dem(output_path="./output")

    # Set bounding box (La Rochelle area, France)
    # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
    dem.set_bbox(-1.182833,46.145232,-1.107044,46.176027)

    # Set CRS (optional, defaults to EPSG:2154)
    dem.set_crs(2154)

    print(f"📦 Bounding box set")
    geo = dem.geo_core
    print(f"🗺️  CRS: {geo.epsg}")

    # Run DEM processing: downloads from IGN API, reprojects, and saves
    print("⏳ Downloading DEM from IGN API...")
    dem = dem.run()

    # Get output paths
    tiff_path = dem.get_path_save_tiff()
    mask_path = dem.get_path_save_mask()

    print(f"✅ DEM processing complete!")
    print(f"📁 DEM TIFF: {tiff_path}")
    print(f"📁 Mask: {mask_path}")

    # Check if files exist
    if os.path.exists(tiff_path):
        size = os.path.getsize(tiff_path) / (1024 * 1024)  # MB
        print(f"📊 DEM file size: {size:.2f} MB")

    if os.path.exists(mask_path):
        print(f"✅ Mask file created")

    return dem


if __name__ == "__main__":
    dem = main()
