"""
Example: Complete COSIA workflow - Download, vectorize and convert to UMEP format

This example demonstrates how to:
1. Download Cosia (landcover) raster from IGN API
2. Vectorize the COSIA raster by RGB color matching
3. Classify polygons into COSIA landcover classes
4. Convert to UMEP landcover classification format
5. Rasterize to UMEP-compatible GeoTIFF

Required dependencies:
    pip install geopandas rasterio numpy shapely
"""

import os
from pathlib import Path

import geopandas as gpd
import numpy as np
import pyproj
import rasterio
from rasterio.features import rasterize, shapes
from rasterio.transform import from_bounds
from shapely.geometry import box, shape

import pymdurs

# ========================================================================
# COSIA Color to Class Mapping
# ========================================================================
from pymdurs.geometric_helpers import TABLE_COLOR_COSIA

# COSIA to UMEP classification mapping
COSIA_TO_UMEP = {
    "Bâtiment": 2,  # Building
    "Zone imperméable": 1,  # Paved
    "Zone perméable": 6,  # Bare Soil
    "Piscine": 7,  # Water
    "Serre": 1,  # Paved
    "Sol nu": 6,  # Bare Soil
    "Surface eau": 7,  # Water
    "Neige": 7,  # Water
    "Conifère": 3,
    "Feuillu": 4,
    "Coupe": 5,  # Grass
    "Broussaille": 5,  # Grass
    "Pelouse": 5,  # Grass
    "Culture": 5,  # Grass
    "Terre labourée": 6,  # Bare Soil
    "Vigne": 5,  # Grass
    "Autre": 1,  # Paved
}

# UMEP labels
UMEP_LABELS = {
    1: "Paved",
    2: "Building",
    3: "Evergreen Trees",
    4: "Deciduous Trees",
    5: "Grass",
    6: "Bare Soil",
    7: "Water",
}


def hex_to_rgb(hex_color):
    """Convert hex color to RGB tuple."""
    hex_color = hex_color.lstrip("#")
    return tuple(int(hex_color[i : i + 2], 16) for i in (0, 2, 4))


def geodataframe_to_tif_with_metadata(
    gdf: gpd.GeoDataFrame,
    output_tif: str,
    column: str = "type",
    resolution: float = 1.0,
):
    """
    Convert a GeoDataFrame to TIF with classification metadata.

    Args:
        gdf: GeoDataFrame with geometries and classification column
        output_tif: Output GeoTIFF path
        column: Column name containing classification values
        resolution: Pixel resolution in meters
    """
    print("\n📊 Converting GeoDataFrame to TIF...")
    print(f"   Column: {column}, Resolution: {resolution} m")

    # Validate GeoDataFrame
    if len(gdf) == 0:
        raise ValueError("GeoDataFrame is empty, cannot create raster")

    bounds = gdf.total_bounds
    print(f"   Bounds: {bounds}")

    # Calculate dimensions
    width = int((bounds[2] - bounds[0]) / resolution)
    height = int((bounds[3] - bounds[1]) / resolution)

    # Validate dimensions
    if width <= 0 or height <= 0:
        raise ValueError(
            f"Invalid dimensions: width={width}, height={height}. "
            f"Bounds: {bounds}, Resolution: {resolution}m. "
            f"Check that resolution is not too large for the extent."
        )

    print(f"   Computed dimensions: {width}x{height} pixels")
    transform = from_bounds(bounds[0], bounds[1], bounds[2], bounds[3], width, height)

    # Rasterize
    shapes_iter = ((geom, value) for geom, value in zip(gdf.geometry, gdf[column]))
    raster = rasterize(
        shapes=shapes_iter,
        out_shape=(height, width),
        transform=transform,
        fill=0,
        dtype=np.uint8,
        all_touched=False,
    )

    # Statistics
    print("\n=== Raster statistics ===")
    print(f"Dimensions: {width}x{height} pixels")
    print(f"Resolution: {resolution} m/pixel")
    print(f"Total area: {(width * height * resolution**2) / 10000:.2f} ha")

    print("\nType distribution:")
    for val in sorted(np.unique(raster)):
        if val != 0:
            count = np.sum(raster == val)
            area_m2 = count * (resolution**2)
            area_ha = area_m2 / 10000
            percentage = (count / raster.size) * 100
            label = UMEP_LABELS.get(val, "Unknown")
            print(f"  {val} - {label:20s}: {area_ha:8.2f} ha ({percentage:5.2f}%)")

    # Save with metadata
    with rasterio.open(
        output_tif,
        "w",
        driver="GTiff",
        height=height,
        width=width,
        count=1,
        dtype=raster.dtype,
        crs=gdf.crs,
        transform=transform,
        compress="lzw",
        nodata=0,
    ) as dst:
        dst.write(raster, 1)

        # Add metadata tags
        dst.update_tags(
            description="COSIA Land Cover Classification (UMEP format)",
            resolution=f"{resolution}m",
            classes=str(UMEP_LABELS),
        )

    print(f"\n✅ File saved: {output_tif}")

    return raster


def vectorize_cosia_raster(cosia_tiff_path: str):
    """
    Vectorize COSIA raster by matching RGB colors to landcover classes.

    Args:
        cosia_tiff_path: Path to COSIA GeoTIFF file

    Returns:
        GeoDataFrame with classified polygons
    """
    print("\n🔍 Vectorizing COSIA raster...")
    print(f"   File: {cosia_tiff_path}")

    # Create RGB to class mapping
    rgb_to_class = {
        hex_to_rgb(color): classe for classe, color in TABLE_COLOR_COSIA.items()
    }

    with rasterio.open(cosia_tiff_path) as src:
        # Read 3 bands (RGB)
        image = src.read()  # Shape: (3, height, width)
        transform = src.transform
        crs = src.crs

        # Combine RGB into single integer per pixel
        # R * 256^2 + G * 256 + B
        combined = (
            (image[0].astype(np.uint32) << 16)
            + (image[1].astype(np.uint32) << 8)
            + image[2].astype(np.uint32)
        )

        # Vectorize
        results = shapes(combined, transform=transform)

        geoms = []
        rgb_values = []

        for geom, value in results:
            # Decode integer to RGB
            value_int = int(value) if isinstance(value, (float, np.floating)) else value
            r = (value_int >> 16) & 255
            g = (value_int >> 8) & 255
            b = value_int & 255

            geoms.append(shape(geom))
            rgb_values.append((r, g, b))

    gdf = gpd.GeoDataFrame({"rgb": rgb_values, "geometry": geoms}, crs=crs)
    print(f"   {len(gdf)} polygons created")

    # Match colors to COSIA classes
    def match_color(rgb):
        """Find closest matching COSIA class by RGB color."""
        min_dist = float("inf")
        best = "Autre"
        for target_rgb, classe in rgb_to_class.items():
            dist = sum((a - b) ** 2 for a, b in zip(rgb, target_rgb))
            if dist < min_dist:
                min_dist = dist
                best = classe
        return best

    gdf["classe"] = gdf["rgb"].apply(match_color)
    gdf["couleur"] = gdf["classe"].map(TABLE_COLOR_COSIA)
    gdf["type"] = gdf["classe"].map(COSIA_TO_UMEP)

    # Drop RGB column
    gdf = gdf.drop(columns=["rgb"])

    print(f"✅ Vectorization complete: {len(gdf)} classified polygons")
    print(f"   Classes found: {gdf['classe'].value_counts().to_dict()}")

    return gdf


def main(output_path: Path):
    """Main workflow: Download COSIA, vectorize, and convert to UMEP format."""
    print("=" * 60)
    print("🌍 COSIA Workflow: Download, Vectorize, and Convert to UMEP")
    print("=" * 60)

    # Bounding box (La Rochelle area, France)
    # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
    #bbox_wgs84 = (-1.152223, 46.183282, -1.149637, 46.185459)
    # bbox_wgs84 = (-1.152704, 46.181627, -1.139893, 46.18699)
    # Tipee
    #bbox_wgs84 = (-1.1484979698,46.1838328061,-1.1444401229,46.1862607901)
    
    # Bordeax
    #bbox_wgs84 = (-0.5833492801,44.8457876761,-0.5737192696,44.8509319773)
    # Atlantec
    bbox_wgs84 = (-1.153414,46.180217,-1.141098,46.186531)
    working_crs = 2154  # Lambert 93

    # Domain boundary GeoJSON export (e.g. for City4CFD domain_bnd)
    domain_geojson_path = output_path / "domainBnd.geojson"
    # Côtés du domaine: None = bbox telle quelle; float = demi-côté en m (carré centré).
    # Pour City4CFD: doit être > influence_region (ex. 400 si influence_region=300).
    domain_half_side_m: float | None = (
        114000  # 800 m de côté; mettre None pour utiliser la bbox
    )

    # Transform bbox WGS84 -> EPSG:2154 for display
    transformer = pyproj.Transformer.from_crs("EPSG:4326", "EPSG:2154", always_xy=True)
    min_x_2154, min_y_2154 = transformer.transform(bbox_wgs84[0], bbox_wgs84[1])
    max_x_2154, max_y_2154 = transformer.transform(bbox_wgs84[2], bbox_wgs84[3])
    bbox_2154 = (min_x_2154, min_y_2154, max_x_2154, max_y_2154)

    # Central point of the simulation (centre du bbox).
    # All points are translated by these coordinates. Must fall in a building polygon
    # if using BPGs for influence region/domain boundaries.
    point_of_interest_wgs84 = (
        (bbox_wgs84[0] + bbox_wgs84[2]) / 2,
        (bbox_wgs84[1] + bbox_wgs84[3]) / 2,
    )
    point_of_interest_2154 = (
        (min_x_2154 + max_x_2154) / 2,
        (min_y_2154 + max_y_2154) / 2,
    )

    # Build domain polygon: bbox or square centered on point_of_interest (côtés configurables)
    if domain_half_side_m is not None:
        cx, cy = point_of_interest_2154
        h = domain_half_side_m
        domain_geom = box(cx - h, cy - h, cx + h, cy + h)
    else:
        domain_geom = box(*bbox_2154)

    domain_gdf = gpd.GeoDataFrame(
        {"id": [1]},
        geometry=[domain_geom],
        crs=f"EPSG:{working_crs}",
    )
    domain_geojson_path.parent.mkdir(parents=True, exist_ok=True)
    domain_gdf.to_file(domain_geojson_path, driver="GeoJSON")

    print("\n📦 Configuration:")
    print(f"   Bounding box (WGS84): {bbox_wgs84}")
    print(f"   Bounding box (EPSG:2154): {bbox_2154}")
    print(f"   point_of_interest (WGS84): {point_of_interest_wgs84}")
    print(f"   point_of_interest (EPSG:2154): {point_of_interest_2154}")
    print(f"   Domain half-side (m): {domain_half_side_m}")
    print(f"   Domain GeoJSON: {domain_geojson_path}")
    print(f"   CRS: EPSG:{working_crs}")
    print(f"   Output folder: {output_path}")

    # ========================================================================
    # Step 1: Download COSIA from IGN API
    # ========================================================================
    print("\n" + "=" * 60)
    print("Step 1: Downloading COSIA from IGN API...")
    print("=" * 60)

    cosia = pymdurs.geometric.Cosia(output_path=str(output_path))
    cosia.set_bbox(*bbox_wgs84)
    cosia.set_crs(working_crs)

    print("⏳ Downloading COSIA from IGN API...")
    cosia = cosia.run_ign()

    cosia_tiff_path = cosia.get_path_save_tiff()
    print(f"✅ COSIA downloaded: {cosia_tiff_path}")

    if os.path.exists(cosia_tiff_path):
        size = os.path.getsize(cosia_tiff_path) / (1024 * 1024)  # MB
        print(f"📊 File size: {size:.2f} MB")

    # ========================================================================
    # Step 2: Vectorize COSIA raster
    # ========================================================================
    print("\n" + "=" * 60)
    print("Step 2: Vectorizing COSIA raster...")
    print("=" * 60)

    gdf = vectorize_cosia_raster(cosia_tiff_path)

    # Save vectorized shapefile
    landcover_shp = output_path / "cosia_landcover.shp"

    # ========================================================================
    # Step 3: Convert to UMEP format and rasterize
    # ========================================================================
    print("\n" + "=" * 60)
    print("Step 3: Converting to UMEP format and rasterizing...")
    print("=" * 60)

    # Convert to working CRS
    gdf = gdf.to_crs(working_crs)

    # Filter valid geometries
    import matplotlib.pyplot as plt

    gdf_valid = gdf[gdf.geometry.notna()].copy()
    fig, ax = plt.subplots(figsize=(10, 10))
    gdf_valid.plot(ax=ax, cmap="viridis", column="type", legend=True)
    ax.set_title("Landcover")
    ax.set_xlabel("Longitude")
    ax.set_ylabel("Latitude")
    ax.set_aspect("equal")
    # plt.show()
    plt.savefig("gdf_valid.png")
    plt.close()

    if len(gdf_valid) == 0:
        print("⚠️  No valid geometry, stopping processing")
        return

    print(f"📊 {len(gdf_valid)} valid geometries out of {len(gdf)} total")

    # landcover_shp = output_path / "roads.shp"
    landcover_shp = output_path / "terrain.shp"
    landcover_geojson = output_path / "terrain.geojson"

    gdf_final = gdf_valid[gdf_valid["type"].isin([6, 1])].copy()
    gdf_final["uniqueid"] = range(1, len(gdf_final) + 1)
    gdf_final.to_file(landcover_shp, driver="ESRI Shapefile")
    gdf_final.to_file(landcover_geojson, driver="GeoJSON")

    print(f"✅ Shapefile saved: {landcover_shp}")

    # Rasterize to UMEP format
    landcover_tif = output_path / "landcover.tif"
    raster = geodataframe_to_tif_with_metadata(
        gdf=gdf_valid,
        output_tif=str(landcover_tif),
        column="type",
        resolution=0.5,  # 1 meter resolution
    )

    # ========================================================================
    # Summary
    # ========================================================================
    print("\n" + "=" * 60)
    print("✅ COSIA workflow complete!")
    print("=" * 60)
    print("📁 Output files:")
    print(f"   - COSIA raster: {cosia_tiff_path}")
    print(f"   - Landcover shapefile: {landcover_shp}")
    print(f"   - UMEP landcover raster: {landcover_tif}")

    return cosia, gdf, raster


if __name__ == "__main__":
    output_folder = "./output/umep_workflow"
    output_path = Path(output_folder).absolute()
    output_path.mkdir(parents=True, exist_ok=True)
    cosia, gdf, raster = main(output_path=output_path)
