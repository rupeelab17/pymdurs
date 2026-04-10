"""
Example: Download LCZ (Local Climate Zone) data from URL using rsmdu

This example demonstrates how to:
1. Create an Lcz instance
2. Set a bounding box
3. Load LCZ data from zip URL
4. Get GeoJSON data
5. Save to GeoJSON file
"""

import pymdurs


def main():
    print("🌡️  Loading LCZ from URL...")

    # Create Lcz instance
    lcz = pymdurs.geometric.Lcz(output_path="./output")

    # Set bounding box (La Rochelle area, France)
    # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
    lcz.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699)

    #lcz.set_bbox(46.181627, -1.152704, 46.18699, -1.139893)
    # Set CRS (optional, defaults to EPSG:2154)
    lcz.set_crs(2154)

    geo = lcz.geo_core
    print(f"📦 Bounding box set")
    print(f"🗺️  CRS: {geo.epsg}")
    print(f"📁 Output path: {geo.output_path}")

    # Display LCZ color table
    print("\n🎨 LCZ Color Table:")
    table_color = lcz.get_table_color()
    for code in sorted(table_color.keys()):
        info = table_color[code]
        print(f"  LCZ {code}: {info['name']} ({info['color']})")

    # Run LCZ processing: loads from zip URL, filters by bbox
    print("\n⏳ Loading LCZ from URL...")
    print("  Note: Full implementation requires GDAL shapefile reading")
    lcz = lcz.run()

    # Get GeoJSON (equivalent to to_gdf() in Python)
    print("📊 Getting GeoJSON data...")
    geojson = lcz.geojson()

    if geojson and "features" in geojson:
        num_features = len(geojson["features"])
        print(f"✅ Loaded {num_features} LCZ zones")

        # Save to GeoJSON
        print("💾 Saving to GeoJSON...")
        lcz.to_geojson(name="lcz")
    else:
        print("⚠️  LCZ processing not yet fully implemented")
        print("   Full implementation requires:")
        print("   - GDAL shapefile reading from zip URLs")
        print("   - Spatial overlay operations")
        print("   - CRS reprojection")
        print("   For now, LCZ structure is ready but data loading is pending.")

    print(f"✅ LCZ processing complete!")
    print(f"📁 Output path: {lcz.get_output_path()}")

    return lcz


if __name__ == "__main__":
    lcz = main()
