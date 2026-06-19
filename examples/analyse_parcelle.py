import os
import sys
from pathlib import Path

import geopandas as gpd
import numpy as np
import rasterio
from rasterio.features import rasterize
import solweig
from shapely.geometry import box

import pymdurs

_EXAMPLES_DIR = Path(__file__).resolve().parent
if str(_EXAMPLES_DIR) not in sys.path:
    sys.path.insert(0, str(_EXAMPLES_DIR))

from utils import warp_clip_raster  # noqa: E402


def add_building_to_dsm(dsm_path, building_path, output_path,
                        building_crs="EPSG:3946", building_height=8):
    with rasterio.open(dsm_path) as src:
        dsm = src.read(1)
        profile = src.profile
        transform = src.transform
        crs = src.crs

    building_gdf = gpd.read_file(building_path)

    building_gdf = building_gdf.set_crs(building_crs)
    building_gdf = building_gdf.to_crs(crs)

    mask = rasterize(
        [(geom, 1) for geom in building_gdf.geometry],
        out_shape=dsm.shape,
        transform=transform,
        fill=0,
        dtype="uint8"
    )

    dsm_modified = dsm.copy()
    dsm_modified[mask == 1] = dsm[mask == 1] + building_height
    with rasterio.open(output_path, "w", **profile) as dst:
        dst.write(dsm_modified, 1)


def add_trees_to_cdsm(cdsm_path, trees_path, output_path,
                      tree_height=8, tree_radius=4, shape="gaussian"):
    with rasterio.open(cdsm_path) as src:
        cdsm = src.read(1).astype(np.float32)
        transform = src.transform
        profile = src.profile.copy()
        res = abs(src.res[0])
        nodata = src.nodata

    # Track where we add trees (to not restore nodata there)
    tree_mask = np.zeros(cdsm.shape, dtype=bool)

    # Replace nodata with 0 (ground level) for processing
    if nodata is not None:
        nodata_mask = (cdsm == nodata) | np.isnan(cdsm)
        cdsm[nodata_mask] = 0
    else:
        nodata_mask = np.isnan(cdsm)
        cdsm[nodata_mask] = 0

    trees = gpd.read_file(trees_path)
    radius_px = int(np.ceil(tree_radius / res))

    # Create kernel
    y, x = np.ogrid[-radius_px:radius_px + 1, -radius_px:radius_px + 1]
    r = np.sqrt(x ** 2 + y ** 2) / radius_px

    if shape == "gaussian":
        kernel = np.exp(-2 * r ** 2)
    elif shape == "cone":
        kernel = np.maximum(0, 1 - r)
    else:
        kernel = np.maximum(0, 1 - r ** 2)
    kernel[r > 1] = 0

    cdsm_new = cdsm.copy()
    added = 0

    for geom in trees.geometry:
        pt = geom.centroid if geom.geom_type != 'Point' else geom
        col = int((pt.x - transform.c) / transform.a)
        row = int((pt.y - transform.f) / transform.e)

        if not (0 <= row < cdsm.shape[0] and 0 <= col < cdsm.shape[1]):
            continue

        ground_elev = cdsm[row, col]  # Now 0 if was nodata

        r1, r2 = max(0, row - radius_px), min(cdsm.shape[0], row + radius_px + 1)
        c1, c2 = max(0, col - radius_px), min(cdsm.shape[1], col + radius_px + 1)
        kr1, kr2 = radius_px - (row - r1), radius_px + (r2 - row)
        kc1, kc2 = radius_px - (col - c1), radius_px + (c2 - col)

        tree_crown = ground_elev + tree_height * kernel[kr1:kr2, kc1:kc2]
        cdsm_new[r1:r2, c1:c2] = np.maximum(cdsm_new[r1:r2, c1:c2], tree_crown)

        # Mark where tree was added
        tree_mask[r1:r2, c1:c2] |= (kernel[kr1:kr2, kc1:kc2] > 0.1)
        added += 1

    # Only restore nodata where NO trees were added
    if nodata is not None:
        restore_nodata = nodata_mask & ~tree_mask
        cdsm_new[restore_nodata] = nodata

    profile.update(dtype=np.float32)
    with rasterio.open(output_path, 'w', **profile) as dst:
        dst.write(cdsm_new, 1)

    print(f"✓ Added {added} trees (height={tree_height}m, radius={tree_radius}m)")
    print(f"✓ Saved to: {output_path}")


def update_landcover(landcover_path, output_path,
                     trees_shp=None, buildings_shp=None,
                     tree_class=4, tree_radius=4, building_class=2):
    with rasterio.open(landcover_path) as src:
        lc = src.read(1)
        transform = src.transform
        profile = src.profile.copy()
        crs = src.crs
        res = abs(src.res[0])

    lc_new = lc.copy()

    # === Add trees ===
    if trees_shp:
        trees = gpd.read_file(trees_shp).to_crs(crs)

        # Buffer points to circles
        trees_buffered = trees.copy()
        trees_buffered.geometry = trees.geometry.buffer(tree_radius)

        tree_mask = rasterize(
            [(g, 1) for g in trees_buffered.geometry],
            out_shape=lc.shape, transform=transform,
            fill=0, dtype=np.uint8
        ) > 0

        lc_new[tree_mask] = tree_class
        print(f"✓ Added {len(trees)} trees as class {tree_class}")

    # === Add buildings ===
    if buildings_shp:
        buildings = gpd.read_file(buildings_shp)

        if buildings.crs is None:
            buildings = buildings.set_crs("EPSG:3946")

        buildings = buildings.to_crs(crs)

        bld_mask = rasterize(
            [(g, 1) for g in buildings.geometry],
            out_shape=lc.shape, transform=transform,
            fill=0, dtype=np.uint8
        ) > 0

        lc_new[bld_mask] = building_class
        print(f"✓ Added {len(buildings)} buildings as class {building_class}")

    # === Save ===
    with rasterio.open(output_path, 'w', **profile) as dst:
        dst.write(lc_new, 1)

    print(f"✓ Saved: {output_path}")

def generate_files(output_folder, bbox_wgs84):


    # Configuration
    output_path = Path(output_folder).absolute()
    output_path.mkdir(parents=True, exist_ok=True)
    output_folder_str = str(output_path)

    # Convert bbox to Lambert-93 (EPSG:2154) with GeoPandas
    minx, miny, maxx, maxy = bbox_wgs84
    geom_wgs84 = box(minx, miny, maxx, maxy)
    gdf_bbox = gpd.GeoDataFrame(geometry=[geom_wgs84], crs="EPSG:4326")
    gdf_bbox = gdf_bbox.to_crs(2154)
    bbox_2154 = tuple(gdf_bbox.total_bounds)  # (minx, miny, maxx, maxy)

    # Working CRS (Lambert 93 - EPSG:2154)
    working_crs = 2154

    print(f"📦 Bounding box: {bbox_wgs84}")
    print(f"🗺️  Working CRS: EPSG:{working_crs}")
    print(f"📁 Output folder: {output_folder_str}")

    # ========================================================================
    # Step 1: Collect DEM from IGN API
    # ========================================================================
    print("\n" + "=" * 60)
    print("Step 1: Collecting DEM from IGN API...")
    print("=" * 60)

    dem = pymdurs.geometric.Dem(output_path=output_folder_str)
    dem.set_bbox(*bbox_wgs84)
    dem.set_crs(working_crs)
    dem = dem.run()

    dem_source = Path(output_folder_str) / "DEM.tif"
    print(f"✅ DEM collected and saved to: {dem_source}")

    # ========================================================================
    # Step 2: Load LiDAR data from IGN WFS service
    # ========================================================================
    dsm_source = Path(output_folder_str) / "DSM.tif"
    if not dsm_source.exists():
        print("\n" + "=" * 60)
        print("Step 2: Loading LiDAR data from IGN WFS service...")
        print("=" * 60)

        # Create Lidar instance
        lidar = pymdurs.geometric.Lidar(output_path=output_folder_str)

        # Set bounding box (same as DEM)
        lidar.set_bbox(*bbox_wgs84)

        # Set CRS (same as DEM)
        lidar.set_crs(working_crs)

        print("📦 Bounding box set")
        geo = lidar.geo_core
        print(f"🗺️  CRS: {geo.epsg}")

        # Generate CDSM from vegetation and water classes
        # Classification: 2 = Ground, 3 = Low Vegetation, 4 = Medium Vegetation,
        #                 5 = High Vegetation, 9 = Water
        print("🌳 Generating CDSM from vegetation and water classes...")
        classification_list = [3, 4, 5]  # Vegetation and water classes
        lidar.run(file_name="CDSM.tif", classification_list=classification_list)
        print("✅ CDSM generated")

        # Generate DSM from ground and buildings classes
        print("🏢 Generating DSM from ground and buildings classes...")
        classification_list = [2, 6, 9]  # Ground and buildings classes
        dsm_output_path = lidar.run(
            file_name="DSM.tif", classification_list=classification_list
        )

        print("✅ LiDAR processing complete!")
        print(f"📁 DSM GeoTIFF saved to: {dsm_output_path}")

        # Check if file exists
        if os.path.exists(dsm_output_path):
            size = os.path.getsize(dsm_output_path) / (1024 * 1024)  # MB
            print(f"📊 DSM GeoTIFF file size: {size:.2f} MB")
            print("📊 File contains 3 bands:")
            print("   - Band 1: DSM (Digital Surface Model)")
            print("   - Band 2: DTM (Digital Terrain Model)")
            print("   - Band 3: CHM (Canopy Height Model)")

    # ========================================================================
    # Step 3: Warp and clip rasters using mask
    # ========================================================================
    print("\n" + "=" * 60)
    print("Step 3: Warping and clipping rasters with mask...")
    print("=" * 60)

    mask_shp_path = Path(output_folder_str) / "mask.shp"
    dem_clip_path = Path(output_folder_str) / "DEM_clip.tif"
    dsm_clip_path = Path(output_folder_str) / "DSM_clip.tif"
    cdsm_clip_path = Path(output_folder_str) / "CDSM_clip.tif"
    cdsm_source = Path(output_folder_str) / "CDSM.tif"
    landcover_clip_path = Path(output_folder_str) / "landcover_clip.tif"
    landcover_source = Path(output_folder_str) / "landcover.tif"

    if mask_shp_path.exists():
        clip_targets = [
            ("DEM", dem_source, dem_clip_path),
            ("DSM", dsm_source, dsm_clip_path),
            ("CDSM", cdsm_source, cdsm_clip_path),
            ("Landcover", landcover_source, landcover_clip_path),
        ]
        for label, src, dst in clip_targets:
            if src.exists():
                warp_clip_raster(src, dst, mask_shp_path)
                print(f"✅ {label} clipped to: {dst}")
            elif label == "Landcover":
                print(
                    "⚠️  landcover.tif missing: run from examples/ first: "
                    "python cosia_from_ign.py"
                )
    else:
        print("⚠️  Mask shapefile not found, skipping clipping")

    if dem_clip_path.exists() and dsm_clip_path.exists():
        print("\n" + "-" * 40)
        print("Step 3a: Filling DSM NoData with DEM values...")
        print("-" * 40)

        with rasterio.open(dem_clip_path) as dem_src:
            dem_data = dem_src.read(1)
            dem_nodata = dem_src.nodata or -99999.0

        with rasterio.open(dsm_clip_path) as dsm_src:
            dsm_data = dsm_src.read(1)
            dsm_profile = dsm_src.profile.copy()
            dsm_nodata = dsm_src.nodata or 0

        # Find DSM NoData pixels
        dsm_invalid = (dsm_data == dsm_nodata) | np.isnan(dsm_data) | (dsm_data == 0)
        dem_valid = (dem_data != dem_nodata) & ~np.isnan(dem_data)

        # Fill DSM NoData with DEM values where DEM is valid
        fill_mask = dsm_invalid & dem_valid
        filled_count = np.sum(fill_mask)

        if filled_count > 0:
            dsm_data[fill_mask] = dem_data[fill_mask]

            # Update nodata value
            dsm_profile.update(nodata=-9999.0)

            # Save filled DSM
            with rasterio.open(dsm_clip_path, "w", **dsm_profile) as dst:
                dst.write(dsm_data, 1)

            print(f"✅ Filled {filled_count} DSM NoData pixels with DEM values")
            print(f"   ({filled_count / dsm_data.size * 100:.2f}% of total)")
        else:
            print("✅ No DSM NoData pixels to fill")

    # Set paths for later steps
    # dsm_path = Path(output_folder_str) / "DSM_clip.tif"
    # cdsm_path = Path(output_folder_str) / "CDSM_clip.tif"
    # dem_clip_path = Path(output_folder_str) / "DEM_clip.tif"
    # lc_path = Path(output_folder_str) / "landcover_clip.tif"


def run_umep(output_path, dsm_path, cdsm_path, lc_path, weather_file, datetime_start, datetime_end):

    if dsm_path and os.path.exists(dsm_path) and lc_path and os.path.exists(lc_path):
        print("\n" + "=" * 60)
        print(" Running SOLWEIG for thermal comfort analysis...")
        print("=" * 60)
        # %%
        # Step 1: Prepare surface data
        # - CRS automatically extracted from DSM
        # - Walls and SVF computed and cached to working_dir if not provided
        # - Extent and resolution handled automatically
        # - Resampled data saved to working_dir for inspection
        surface = solweig.SurfaceData.prepare(
            dsm=str(dsm_path),
            working_dir=str(output_path / "working"),  # Cache preprocessing here
            cdsm=str(cdsm_path),
            # bbox=bbox_2154,  # Optional: specify extent
            pixel_size=1.0,  # Optional: specify resolution (default: from DSM),
            land_cover=str(lc_path),  # Grid with class IDs (0-7, 99-102),
            cdsm_relative=False,
        )

        # Load weather from EPW fileVectorized COSIA
        weather_list = solweig.Weather.from_epw(
            weather_file, start = datetime_start, end = datetime_end
        )
        physics = solweig.load_physics("physics_defaults.json")
        materials = solweig.load_materials("default_materials.json")
        config = solweig.ModelConfig.defaults()
        config.save("my_config.json")

        # Calculate timeseries
        results = solweig.calculate(
            surface=surface,
            physics=physics,
            materials=materials,
            human=solweig.HumanParams(
                abs_k=0.65,  # Lower shortwave absorption
                abs_l=0.97,  # Higher longwave absorption
                weight=70,  # 70 kg
                height=1.65,  # 165 cmrm
                posture="standing",
            ),
            weather=weather_list,
            use_anisotropic_sky=True,  # Uses SVF (computed automatically if needed)
            conifer=False,  # Use seasonal leaf on/off (set True for evergreen trees)
            output_dir=str(output_path),
            outputs=["tmrt", "shadow", "utci"],
        )
        print("✅ SOLWEIG run complete!")

        print(results.report())

        # %%
        # Plot timeseries (Ta, Tmrt, UTCI, radiation, sun exposure over time)
        results.plot()


if __name__ == "__main__":
    input_folder = "./output/data_parcelle"
    input_path = Path(input_folder).absolute()
    input_folder_str = str(input_path)

    BUILDINGS_SHP = Path(input_folder_str) / "building.shp"  # Set to None to skip
    BUILDING_CLASS = 2
    BUILDING_HEIGHT = 9.0  # meters
    BUILDING_CRS = "EPSG:3946"

    TREES_SHP = Path(input_folder_str) / "arbres.shp"  # Set to None to skip
    TREE_HEIGHT = 8.0  # meters
    TREE_RADIUS = 4.0  # meters
    TREE_SHAPE = "gaussian"  # "gaussian", "cone", or "paraboloid"
    TREE_CLASS = 4  # 3=Evergreen, 4=Deciduous
    TREE_RADIUS = 4.0  # Crown radius in meters (for point centroids)


    output_folder = "./output/umep_parcelle"
    output_path = Path(output_folder).absolute()
    output_folder_str = str(output_path)

    # Bounding box (La Rochelle area, France)
    # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
    bbox_wgs84 = (-1.1588496989615957, 46.18911836553419, -1.1545470910384044, 46.19234901409732)

    weather_file =  "la_rochelle_2025.epw"
    datetime_start = "2025-06-21 10:00"
    datetime_end = "2025-06-21 14:00"

    generate_files(output_folder, bbox_wgs84)

    DSM_PATH =  Path(output_folder_str) / "DSM_clip.tif"
    CDSM_PATH =  Path(output_folder_str) / "CDSM_clip.tif"
    LANDCOVER_PATH =  Path(output_folder_str) / "landcover_clip.tif"

    output_folder_avant = "./output/umep_parcelle_avant"
    output_path_avant = Path(output_folder_avant).absolute()
    output_path_avant.mkdir(parents=True, exist_ok=True)

    run_umep(output_path_avant, DSM_PATH, CDSM_PATH, LANDCOVER_PATH, weather_file, datetime_start,
             datetime_end)

    OUTPUT_PATH_LC =  Path(output_folder_str) / "landcover_updated.tif"
    OUTPUT_PATH_DSM =  Path(output_folder_str) / "DSM_updated.tif"
    OUTPUT_PATH_CDSM =  Path(output_folder_str) / "CDSM_updated.tif"

    add_building_to_dsm(DSM_PATH, BUILDINGS_SHP, OUTPUT_PATH_DSM,
                        BUILDING_CRS, BUILDING_HEIGHT)

    add_trees_to_cdsm(CDSM_PATH, TREES_SHP, OUTPUT_PATH_CDSM,
                      TREE_HEIGHT, TREE_RADIUS, TREE_SHAPE)

    update_landcover(
        LANDCOVER_PATH, OUTPUT_PATH_LC,
        TREES_SHP, BUILDINGS_SHP,
        TREE_CLASS, TREE_RADIUS, BUILDING_CLASS
    )

    output_folder_apres = "./output/umep_parcelle_apres"
    output_path_apres = Path(output_folder_apres).absolute()
    output_path_apres.mkdir(parents=True, exist_ok=True)

    run_umep(output_path_apres, OUTPUT_PATH_DSM, OUTPUT_PATH_CDSM, OUTPUT_PATH_LC, weather_file, datetime_start, datetime_end)

