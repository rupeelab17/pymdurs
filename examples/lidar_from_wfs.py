"""
Example: Download and process LiDAR data from IGN WFS service using pymdurs

This example demonstrates how to:
1. Create a Lidar instance
2. Set a bounding box
3. Download LAZ files from IGN WFS service
4. Process points to create DSM, DTM, and CHM rasters
5. Save results as a multi-band GeoTIFF file
6. Export the point cloud (ROI) as a .las file
"""

import os

import pymdurs


def main():
    print("🛰️  Loading LiDAR data from IGN WFS service...")

    # Create Lidar instance
    # Following Python: lidar = Lidar(output_path="./", classification=6)
    lidar = pymdurs.geometric.Lidar(output_path="./output")

    # Set bounding box (La Rochelle area, France)
    # Load points (required for save_las)
    lidar.set_bbox(-1.152223, 46.183282, -1.149637, 46.185459)
    #    lidar.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699)

    # Set CRS (optional, defaults to EPSG:2154)
    lidar.set_crs(2154)

    print(f"📦 Bounding box set")
    geo = lidar.geo_core
    print(f"🗺️  CRS: {geo.epsg}")

    # Optional: Set classification filter
    # Following Python: classification_list=[3, 4, 5, 9]
    # 1 = unclassified, 2 = Ground, 3 = Low Vegetation, 4 = Medium Vegetation, 5 = High Vegetation, 9 = Water
    classification_list = [3, 4, 5]  # Vegetation and water classes
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
        print(f"⚠️  Could not export LAS (ensure set_bbox loaded points): {e}")
        las_path = None

    print(f"✅ LiDAR processing complete!")
    print(f"📁 GeoTIFF saved to: {output_path}")

    # Check if file exists
    if os.path.exists(output_path):
        size = os.path.getsize(output_path) / (1024 * 1024)  # MB
        print(f"📊 GeoTIFF file size: {size:.2f} MB")
        print(f"📊 File contains 3 bands:")
        print(f"   - Band 1: DSM (Digital Surface Model)")
        print(f"   - Band 2: DTM (Digital Terrain Model)")
        print(f"   - Band 3: CHM (Canopy Height Model)")

    return lidar, output_path


if __name__ == "__main__":
    lidar, output_path = main()
    print(f"\n✨ Example completed successfully!")
    print(f"   You can now open {output_path} in a GIS application like QGIS.")
