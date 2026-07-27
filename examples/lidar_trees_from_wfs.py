"""
Example: Extract tree points (H, D, LAI) from IGN LiDAR via pymdurs.

Pipeline:
1. Download / process LiDAR for a bbox → multi-band GeoTIFF (DSM, DTM, CHM)
2. Watershed segmentation on CHM → one point per tree
3. Write ESRI Shapefile in EPSG:2154 with fields H, D, LAI
"""

from pathlib import Path

import pymdurs
from pymdurs.trees import run_trees


def main():
    output = Path("./output/lidar_trees")
    output.mkdir(parents=True, exist_ok=True)

    lidar = pymdurs.geometric.Lidar(output_path=str(output))
    # La Rochelle sample bbox (WGS84)
    lidar.set_bbox(-1.152223, 46.183282, -1.149637, 46.185459)
    lidar.set_crs(2154)

    shp_path = run_trees(
        lidar,
        file_name="trees.shp",
        classification_list=[2, 3, 4, 5],
        resolution=1.0,
        min_tree_height=2.0,
        min_distance=2,
        lai=4.0,
    )

    print(f"Tree shapefile written to: {shp_path}")

    import geopandas as gpd

    gdf = gpd.read_file(shp_path)
    print(f"Trees: {len(gdf)}")
    print(f"CRS: {gdf.crs}")
    print(f"Columns: {list(gdf.columns)}")
    if len(gdf):
        print(gdf[["H", "D", "LAI"]].describe())

    return shp_path


if __name__ == "__main__":
    main()
