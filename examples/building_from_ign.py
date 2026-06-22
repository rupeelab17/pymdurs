"""
Example: Load buildings from IGN API using rsmdu

This example demonstrates how to:
1. Create a BuildingCollection
2. Set a bounding box
3. Download and process buildings from IGN API
4. Convert to pandas DataFrame
"""

import geopandas as gpd

import pymdurs


def main():
    print("🏢 Loading buildings from IGN API...")

    # Create BuildingCollection (using alias created by rsmdu_helper)
    buildings = pymdurs.geometric.Building(
        output_path="./output", defaultStoreyHeight=3.0
    )
     # Atlantec
    bbox_wgs84 = (-1.153414,46.180217,-1.141098,46.186531)

    # Set bounding box (La Rochelle area, France)
    buildings.set_bbox(*bbox_wgs84)
    # buildings.set_bbox(-1.148001, 46.184158, -1.145528, 46.185264)

    geo = buildings.geo_core
    print("📦 Bounding box set")
    print(f"📁 Output path: {geo.output_path}")

    # Run processing: downloads from IGN API and processes heights
    print("⏳ Downloading buildings from IGN API...")
    buildings = buildings.run()

    print(f"✅ Loaded {len(buildings)} buildings")

    # Convert to pandas DataFrame
    print("📊 Converting to pandas DataFrame...")
    df = buildings.to_pandas()

    # Convert GeoJSON to GeoDataFrame
    print("🗺️ Converting GeoJSON to GeoDataFrame...")
    geojson = buildings.get_geojson()
    gdf = gpd.GeoDataFrame.from_features(geojson.get("features", []), crs="EPSG:4326")

    print(f"✅ GeoDataFrame created with {len(gdf)} features")
    print(f"📊 GeoDataFrame columns: {list(gdf.columns)}")
    print(f"📊 GeoDataFrame CRS: {gdf.crs}")
    gdf = gdf.to_crs(epsg=2154)

    gdf.to_file("buildings.shp", driver="ESRI Shapefile")
    gdf.to_file("buildings.gpkg", driver="GPKG")
    gdf.to_file("buildings.geojson", driver="GeoJSON")

    if geojson and "features" in geojson:
        num_features = len(geojson["features"])
        print(f"✅ Loaded {num_features} buildings")
    else:
        print("✅ Buildings data loaded")

    print("\n📈 DataFrame info:")
    print(df.info())
    print("\n📊 First few rows:")
    print(df.head())
    print("\n📊 Statistics:")
    print(df.describe())

    print("\n🗺️ GeoDataFrame info:")
    print(gdf.info())
    print("\n🗺️ GeoDataFrame first few rows:")
    print(gdf.head())

    return buildings, df, gdf


if __name__ == "__main__":
    buildings, df, gdf = main()
