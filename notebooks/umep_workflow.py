import marimo

__generated_with = "0.19.6"
app = marimo.App()


@app.cell
def _():
    import marimo as mo

    return (mo,)


@app.cell
def _(mo):
    mo.md(r"""
    ## Example: Complete UMEP workflow using pymdurs and solweig

    This example demonstrates how to:
    1. Collect urban data using pymdurs (DEM, buildi    ngs, vegetation)
    2. Process data for UMEP analysis (DSM, CDSM)
    3. Calculate Sky View Factor (SVF) using solweig
    4. Optionally run SOLWEIG for thermal comfort analysis

    Inspired by: https://github.com/UMEP-dev/umep-rust/blob/main/demos/athens-demo.py

    Required dependencies (install separately):
    ```bash
    pip install geopandas rasterio pyproj
    ```
    ### umep (optional) for additional SOLWEIG features
    ```bash
    pip install "solweig @ git+https://github.com/UMEP-dev/solweig.git@main"
    ```
    Note: On Apple Silicon (ARM64), solweig may require the x86_64 target:
    ```bash
    rustup target add x86_64-apple-darwin
    ```
    """)
    return


@app.cell
def _():
    import os
    from pathlib import Path

    import geopandas as gpd
    import numpy as np
    import rasterio
    from osgeo import gdal, gdalconst
    from rasterio.features import rasterize, shapes
    from rasterio.transform import from_bounds
    from shapely.geometry import shape

    import pymdurs

    # Try to import solweig for SVF calculation
    try:
        from umepr import solweig_runner_rust, svf

        HAS_UMEPR = True
    except ImportError:
        HAS_UMEPR = False
        print("⚠️  umepr package not available. SVF calculation will be skipped.")
        print(
            "   Install with: pip install 'solweig @ git+https://github.com/UMEP-dev/solweig.git@main'"
        )
        print(
            "   On Apple Silicon, you may need: rustup target add x86_64-apple-darwin"
        )

    # Try to import umep for additional utilities (optional)
    try:
        from umep import wall_heightaspect_algorithm  # noqa: F401
        from umep.functions.SOLWEIGpython import solweig_runner_core

        HAS_UMEP = True
    except ImportError:
        HAS_UMEP = False
        print("⚠️  umep package not available. Some features will be disabled.")

    print("HAS_UMEPR", HAS_UMEPR)
    print("HAS_UMEP", HAS_UMEP)

    # ========================================================================
    # COSIA Color to Class Mapping
    # ========================================================================
    TABLE_COLOR_COSIA = {
        "Bâtiment": "#ce7079",
        "Zone imperméable": "#a6aab7",
        "Zone perméable": "#987752",
        "Piscine": "#62d0ff",
        "Serre": "#b9e2d4",
        "Sol nu": "#bbb096",
        "Surface eau": "#3375a1",
        "Neige": "#e9effe",
        "Conifère": "#216e2e",
        "Feuillu": "#4c9129",
        "Coupe": "#e48e4d",
        "Broussaille": "#b5c335",
        "Pelouse": "#8cd76a",
        "Culture": "#decf55",
        "Terre labourée": "#d0a349",
        "Vigne": "#b08290",
        "Autre": "#222222",
    }

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
        "Conifère": 6,  # Bare Soil (trees not in UMEP, mapped to soil)
        "Feuillu": 6,  # Bare Soil (trees not in UMEP, mapped to soil)
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
        print("\n📊 Conversion du GeoDataFrame en TIF...")
        print(f"   Colonne: {column}, Résolution: {resolution} m")

        # Validate GeoDataFrame
        if len(gdf) == 0:
            raise ValueError("Le GeoDataFrame est vide, impossible de créer un raster")

        bounds = gdf.total_bounds
        print(f"   Bounds: {bounds}")

        # Calculate dimensions
        width = int((bounds[2] - bounds[0]) / resolution)
        height = int((bounds[3] - bounds[1]) / resolution)

        # Validate dimensions
        if width <= 0 or height <= 0:
            raise ValueError(
                f"Dimensions invalides: width={width}, height={height}. "
                f"Bounds: {bounds}, Résolution: {resolution}m. "
                f"Vérifiez que la résolution n'est pas trop grande par rapport à l'étendue."
            )

        print(f"   Dimensions calculées: {width}x{height} pixels")
        transform = from_bounds(
            bounds[0], bounds[1], bounds[2], bounds[3], width, height
        )

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
        print("\n=== Statistiques du raster ===")
        print(f"Dimensions: {width}x{height} pixels")
        print(f"Résolution: {resolution} m/pixel")
        print(f"Superficie totale: {(width * height * resolution**2) / 10000:.2f} ha")

        print("\nRépartition des types:")
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

        print(f"\n✅ Fichier sauvegardé: {output_tif}")

        return raster

    def vectorize_cosia_raster(cosia_tiff_path: str):
        """
        Vectorize COSIA raster by matching RGB colors to landcover classes.

        Args:
            cosia_tiff_path: Path to COSIA GeoTIFF file

        Returns:
            GeoDataFrame with classified polygons
        """
        print("\n🔍 Vectorisation du raster COSIA...")
        print(f"   Fichier: {cosia_tiff_path}")

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
                value_int = (
                    int(value) if isinstance(value, (float, np.floating)) else value
                )
                r = (value_int >> 16) & 255
                g = (value_int >> 8) & 255
                b = value_int & 255

                geoms.append(shape(geom))
                rgb_values.append((r, g, b))

        gdf = gpd.GeoDataFrame({"rgb": rgb_values, "geometry": geoms}, crs=crs)
        print(f"   {len(gdf)} polygones créés")

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

        print(f"✅ Vectorisation terminée: {len(gdf)} polygones classifiés")
        print(f"   Classes trouvées: {gdf['classe'].value_counts().to_dict()}")

        return gdf

    def run_cosia(output_path: Path):
        """Main workflow: Download COSIA, vectorize, and convert to UMEP format."""
        print("=" * 60)
        print("🌍 COSIA Workflow: Download, Vectorize, and Convert to UMEP")
        print("=" * 60)

        # Bounding box (La Rochelle area, France)
        # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
        bbox_wgs84 = (-1.152704, 46.181627, -1.139893, 46.18699)
        working_crs = 2154  # Lambert 93

        print("\n📦 Configuration:")
        print(f"   Bounding box: {bbox_wgs84}")
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
        gdf.to_file(landcover_shp, driver="ESRI Shapefile")
        print(f"✅ Shapefile saved: {landcover_shp}")

        # ========================================================================
        # Step 3: Convert to UMEP format and rasterize
        # ========================================================================
        print("\n" + "=" * 60)
        print("Step 3: Converting to UMEP format and rasterizing...")
        print("=" * 60)

        # Convert to working CRS
        gdf = gdf.to_crs(working_crs)

        # Filter valid geometries
        gdf_valid = gdf[gdf.geometry.notna()].copy()
        if len(gdf_valid) == 0:
            print("⚠️  Aucune géométrie valide, arrêt du traitement")
            return

        print(f"📊 {len(gdf_valid)} géométries valides sur {len(gdf)} totales")

        # Rasterize to UMEP format
        landcover_tif = output_path / "landcover.tif"
        raster = geodataframe_to_tif_with_metadata(
            gdf=gdf_valid,
            output_tif=str(landcover_tif),
            column="type",
            resolution=1.0,  # 1 meter resolution
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

    def fill_nodata_nan(tif_path, nodata_fallback=-9999):
        """Replace NaN with nodata in a GeoTIFF so SOLWEIG (int conversion) does not fail."""
        path = Path(tif_path)
        if not path.exists():
            return
        with rasterio.open(path, "r+") as src:
            nd = src.nodata if src.nodata is not None else nodata_fallback
            for i in range(1, src.count + 1):
                band = src.read(i)
                if np.issubdtype(band.dtype, np.floating) and np.any(np.isnan(band)):
                    band = np.where(np.isnan(band), nd, band)
                    src.write(band, i)
            src.nodata = nd

    def run_umep(output_path):
        print("🌆 Starting UMEP workflow with pymdurs and umepr...")
        print("=" * 60)

        # Configuration
        output_folder = output_path
        output_path = Path(output_folder).absolute()
        output_path.mkdir(parents=True, exist_ok=True)
        output_folder_str = str(output_path)

        # Bounding box (La Rochelle area, France)
        # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
        bbox_wgs84 = (-1.152704, 46.181627, -1.139893, 46.18699)

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
            classification_list = [3, 4, 5, 9]  # Vegetation and water classes
            lidar.run(file_name="CDSM.tif", classification_list=classification_list)
            print("✅ CDSM generated")

            # Generate DSM from ground and buildings classes
            print("🏢 Generating DSM from ground and buildings classes...")
            classification_list = [2, 6]  # Ground and buildings classes
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
        if mask_shp_path.exists():
            warp_options = gdal.WarpOptions(
                multithread=True,
                format="GTiff",
                xRes=1,
                yRes=1,
                outputType=gdalconst.GDT_Float32,
                dstNodata=None,
                dstSRS="EPSG:2154",
                cropToCutline=True,
                cutlineDSName=str(mask_shp_path),
                cutlineLayer="mask",
            )

            # Clip DEM
            dem_clip_path = Path(output_folder_str) / "DEM_clip.tif"
            if dem_source.exists():
                gdal.Warp(
                    destNameOrDestDS=str(dem_clip_path),
                    srcDSOrSrcDSTab=str(dem_source),
                    options=warp_options,
                )
                print(f"✅ DEM clipped to: {dem_clip_path}")

            # Clip DSM
            dsm_clip_path = Path(output_folder_str) / "DSM_clip.tif"
            if dsm_source.exists():
                gdal.Warp(
                    destNameOrDestDS=str(dsm_clip_path),
                    srcDSOrSrcDSTab=str(dsm_source),
                    options=warp_options,
                )
                print(f"✅ DSM clipped to: {dsm_clip_path}")

            # Clip CDSM
            cdsm_clip_path = Path(output_folder_str) / "CDSM_clip.tif"
            cdsm_source = Path(output_folder_str) / "CDSM.tif"
            if cdsm_source.exists():
                gdal.Warp(
                    destNameOrDestDS=str(cdsm_clip_path),
                    srcDSOrSrcDSTab=str(cdsm_source),
                    options=warp_options,
                )
                print(f"✅ CDSM clipped to: {cdsm_clip_path}")

            # Clip Landcover
            landcover_clip_path = Path(output_folder_str) / "landcover_clip.tif"
            landcover_source = Path(output_folder_str) / "landcover.tif"
            if landcover_source.exists():
                gdal.Warp(
                    destNameOrDestDS=str(landcover_clip_path),
                    srcDSOrSrcDSTab=str(landcover_source),
                    options=warp_options,
                )
                print(f"✅ Landcover clipped to: {landcover_clip_path}")
        else:
            print("⚠️  Mask shapefile not found, skipping clipping")

        # Set paths for later steps
        dsm_path = Path(output_folder_str) / "DSM_clip.tif"
        cdsm_path = Path(output_folder_str) / "CDSM_clip.tif"
        dem_tiff_path = Path(output_folder_str) / "DEM_clip.tif"
        lc_path = Path(output_folder_str) / "landcover_clip.tif"

        # ========================================================================
        # Step 4: Calculate Sky View Factor (SVF) using umepr
        # ========================================================================
        print("\n" + "=" * 60)
        print("Step 4: Calculating Sky View Factor (SVF) using umepr...")
        print("=" * 60)

        if not HAS_UMEPR:
            print("⚠️  Skipping SVF calculation - umepr not available")
        elif dsm_path and os.path.exists(dsm_path):
            # Get bounding box from the actual raster bounds
            # This ensures the bbox matches exactly with the raster extent
            with rasterio.open(dsm_path) as dsm_src:
                # Get the actual bounds of the raster
                bounds = dsm_src.bounds
                total_extents = [bounds.left, bounds.bottom, bounds.right, bounds.top]

            print(f"📐 Using raster bounds for SVF: {total_extents}")

            svf_output_dir = str(output_path / "svf")
            os.makedirs(svf_output_dir, exist_ok=True)

            try:
                svf.generate_svf(
                    dsm_path=dsm_path,
                    bbox=total_extents,
                    out_dir=svf_output_dir,
                    cdsm_path=cdsm_path
                    if cdsm_path and os.path.exists(cdsm_path)
                    else None,
                    trunk_ratio_perc=25,
                    trans_veg_perc=3,  # 3% transmissivity for vegetation
                    use_tiled_loading=False,
                )
                print(f"✅ SVF calculation complete! Output in: {svf_output_dir}")
            except Exception as e:
                print(f"⚠️  SVF calculation failed: {e}")
                print(f"   Error details: {type(e).__name__}: {e}")
        elif dsm_path and os.path.exists(dsm_path):
            print("⚠️  Skipping SVF calculation - umepr not available")
        else:
            print("⚠️  Skipping SVF calculation - DSM not available")

        # ========================================================================
        # Step 5: Generate wall heights for SOLWEIG
        # ========================================================================
        if HAS_UMEP and dsm_path and os.path.exists(dsm_path):
            print("\n" + "=" * 60)
            print("Step 5: Generating wall heights for SOLWEIG...")
            print("=" * 60)

            # Get bounding box from the actual raster bounds
            with rasterio.open(dsm_path) as dsm_src:
                bounds = dsm_src.bounds
                total_extents = [bounds.left, bounds.bottom, bounds.right, bounds.top]

            try:
                walls_output_dir = str(output_path / "walls")
                os.makedirs(walls_output_dir, exist_ok=True)

                wall_heightaspect_algorithm.generate_wall_hts(
                    dsm_path=dsm_path,
                    bbox=total_extents,
                    out_dir=walls_output_dir,
                )
                print(f"✅ Wall heights generated! Output in: {walls_output_dir}")
            except Exception as e:
                print(f"⚠️  Wall height generation failed: {e}")

        # ========================================================================
        # Step 6: Run SOLWEIG for thermal comfort analysis
        # ========================================================================
        if HAS_UMEPR and dsm_path and os.path.exists(dsm_path):
            print("\n" + "=" * 60)
            print("Step 6: Running SOLWEIG for thermal comfort analysis...")
            print("=" * 60)

            # Replace NaN with nodata in rasters to avoid "cannot convert float NaN to integer"
            for rast in (dsm_path, cdsm_path, dem_tiff_path):
                if rast and os.path.exists(rast):
                    fill_nodata_nan(rast)

            SRR = solweig_runner_rust.SolweigRunRust(
                "configsolweig.ini",
                "parametersforsolweig.json",
                use_tiled_loading=False,
                tile_size=1024,
            )
            SRR.run()
            print("✅ SOLWEIG run complete!")
        else:
            print("⚠️  Skipping SOLWEIG - missing requirements or DSM not available")

        # ========================================================================
        # Summary
        # ========================================================================
        print("\n" + "=" * 60)
        print("✅ UMEP workflow complete!")
        print("=" * 60)
        print(f"📁 All outputs saved to: {output_folder_str}")
        print("\nGenerated files:")
        if dsm_path and os.path.exists(dsm_path):
            print(f"  - DSM: {dsm_path}")
        if cdsm_path and os.path.exists(cdsm_path):
            print(f"  - CDSM: {cdsm_path}")
        if dem_tiff_path.exists():
            print(f"  - DEM: {dem_tiff_path}")
        if os.path.exists(str(output_path / "svf")):
            print(f"  - SVF: {output_path / 'svf'}")
        if HAS_UMEP and os.path.exists(str(output_path / "walls")):
            print(f"  - Wall heights: {output_path / 'walls'}")

    def _main_():
        output_folder = "./output/umep_workflow"
        output_path = Path(output_folder).absolute()
        output_path.mkdir(parents=True, exist_ok=True)
        cosia, gdf, raster = run_cosia(output_path=output_path)
        run_umep(output_path=output_folder)

    _main_()
    return (rasterio,)


@app.cell
def _(mo):
    mo.md(r"""
    ## Visualisation Tmrt (Mean Radiant Temperature)

    Affichage du raster Tmrt_average.tif généré par SOLWEIG.
    """)
    return


@app.cell
def _(rasterio):
    from matplotlib import pyplot as plt
    from rasterio.plot import show

    fig, ax = plt.subplots(figsize=(15, 15))
    src = rasterio.open("output/Tmrt_average.tif")
    plt.imshow(src.read(1), cmap="pink")
    show(src, ax=ax, cmap="plasma")
    return


if __name__ == "__main__":
    app.run()
