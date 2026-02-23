"""
Example: Complete UMEP workflow using pymdurs and solweig

This example demonstrates how to:
1. Collect urban data using pymdurs (DEM, buildings, vegetation)
2. Process data for UMEP analysis (DSM, CDSM)
3. Calculate Sky View Factor (SVF) using solweig
4. Optionally run SOLWEIG for thermal comfort analysis

Inspired by: https://github.com/UMEP-dev/solweig/blob/dev/demos/athens-demo.py

Required dependencies (install separately):
    pip install geopandas rasterio pyproj pillow
    pip install "solweig @ git+https://github.com/UMEP-dev/solweig.git@main"
    # umep (optional) for additional SOLWEIG features

Note: On Apple Silicon (ARM64), solweig may require the x86_64 target:
    rustup target add x86_64-apple-darwin
"""

import os
from pathlib import Path

import geopandas as gpd
import numpy as np
import rasterio
import solweig
from osgeo import gdal, gdalconst
from PIL import Image
from shapely.geometry import box

import pymdurs


def preview_pngs_to_gif(
    folder: str | Path,
    pattern: str = "shadow_*.preview.png",
    out_path: str | Path | None = None,
    duration_ms: int = 500,
    loop: int = 0,
) -> Path:
    """Create an animated GIF from SOLWEIG preview PNGs (shadow, tmrt, utci, pet).

    Args:
        folder: Folder containing shadow_*.preview.png (or other pattern) files.
        pattern: Glob pattern for PNG files (default: shadow_*.preview.png).
        out_path: Output GIF file (default: folder / "shadow_preview.gif").
        duration_ms: Duration per frame in ms.
        loop: 0 = loop forever.

    Returns:
        Path to the created GIF.
    """
    folder = Path(folder)
    out_path = Path(out_path) if out_path else folder / "shadow_preview.gif"
    paths = sorted(folder.glob(pattern))
    if not paths:
        raise FileNotFoundError(f"No files found: {folder / pattern}")
    frames = [Image.open(p).convert("P", palette=Image.ADAPTIVE) for p in paths]
    frames[0].save(
        out_path,
        save_all=True,
        append_images=frames[1:],
        duration=duration_ms,
        loop=loop,
    )
    return out_path


def main():
    print("🌆 Starting UMEP workflow with pymdurs and solweig...")
    print("=" * 60)

    # Configuration
    output_folder = "./output/umep_workflow"
    output_path = Path(output_folder).absolute()
    output_path.mkdir(parents=True, exist_ok=True)
    output_folder_str = str(output_path)

    # Bounding box (La Rochelle area, France)
    # Format: min_x, min_y, max_x, max_y (WGS84, EPSG:4326)
    bbox_wgs84 = (-1.152704, 46.181627, -1.139893, 46.18699)

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
            print(
                "⚠️  landcover.tif missing: run from examples/ first: "
                "python cosia_from_ign.py"
            )
    else:
        print("⚠️  Mask shapefile not found, skipping clipping")

    # Set paths for later steps
    dsm_path = Path(output_folder_str) / "DSM_clip.tif"
    cdsm_path = Path(output_folder_str) / "CDSM_clip.tif"
    dem_tiff_path = Path(output_folder_str) / "DEM_clip.tif"
    lc_path = Path(output_folder_str) / "landcover_clip.tif"

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

    # ========================================================================
    # Step 6: Run SOLWEIG for thermal comfort analysis
    # ========================================================================
    if dsm_path and os.path.exists(dsm_path) and lc_path and os.path.exists(lc_path):
        print("\n" + "=" * 60)
        print("Step 6: Running SOLWEIG for thermal comfort analysis...")
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
        )

        # Load weather from EPW fileVectorized COSIA
        weather_list = solweig.Weather.from_epw(
            "la_rochelle_2025.epw",
            start="2025-07-01 07:00",
            end="2025-07-01 19:00",
        )
        physics = solweig.load_physics("parametersforsolweig.json")
        # Calculate timeseries
        results = solweig.calculate_timeseries(
            surface=surface,
            physics=physics,
            human=solweig.HumanParams(
                abs_k=0.65,  # Lower shortwave absorption
                abs_l=0.97,  # Higher longwave absorption
                weight=70,  # 70 kg
                height=1.65,  # 165 cmrm
                posture="sitting",
            ),
            weather_series=weather_list,
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

        # %%
        # Visualise summary grids
        import matplotlib.pyplot as plt  # noqa: E402

        fig, axes = plt.subplots(2, 3, figsize=(15, 10))

        im0 = axes[0, 0].imshow(results.tmrt_mean, cmap="hot")
        axes[0, 0].set_title("Mean Tmrt (°C)")
        plt.colorbar(im0, ax=axes[0, 0])

        im1 = axes[0, 1].imshow(results.utci_mean, cmap="hot")
        axes[0, 1].set_title("Mean UTCI (°C)")
        plt.colorbar(im1, ax=axes[0, 1])

        im2 = axes[0, 2].imshow(results.sun_hours, cmap="YlOrRd")
        axes[0, 2].set_title("Sun hours")
        plt.colorbar(im2, ax=axes[0, 2])

        im3 = axes[1, 0].imshow(results.tmrt_day_mean, cmap="hot")
        axes[1, 0].set_title("Mean daytime Tmrt (°C)")
        plt.colorbar(im3, ax=axes[1, 0])

        im4 = axes[1, 1].imshow(results.tmrt_night_mean, cmap="cool")
        axes[1, 1].set_title("Mean nighttime Tmrt (°C)")
        plt.colorbar(im4, ax=axes[1, 1])

        # Show hours above the first day threshold (32°C by default)
        threshold = sorted(results.utci_hours_above.keys())[0]
        im5 = axes[1, 2].imshow(results.utci_hours_above[threshold], cmap="Reds")
        axes[1, 2].set_title(f"UTCI hours > {threshold}°C")
        plt.colorbar(im5, ax=axes[1, 2])

        for ax in axes.flat:
            ax.set_xticks([])
            ax.set_yticks([])

        plt.suptitle(
            f"SOLWEIG Summary — {len(results)} timesteps ({results.n_daytime} day, {results.n_nighttime} night)"
        )
        plt.tight_layout()
        plt.show()

        # %%
        # Optional: Load and inspect run metadata
        # This metadata captures all parameters used in the calculation for reproducibility
        metadata = solweig.load_run_metadata(output_path / "run_metadata.json")
        print("\nRun metadata loaded:")
        print(f"  Timestamp: {metadata['run_timestamp']}")
        print(f"  SOLWEIG version: {metadata['solweig_version']}")
        print(
            f"  Location: {metadata['location']['latitude']:.2f}°N, {metadata['location']['longitude']:.2f}°E"
        )
        print(
            f"  Human posture: {metadata.get('human', {}).get('posture', 'default (standing)')}"
        )
        print(f"  Anisotropic sky: {metadata['parameters']['use_anisotropic_sky']}")
        print(f"  Weather timesteps: {metadata['timeseries']['timesteps']}")
        print(
            f"  Date range: {metadata['timeseries']['start']} to {metadata['timeseries']['end']}"
        )
    else:
        print("⚠️  Skipping SOLWEIG - missing requirements or DSM not available")

    # ========================================================================
    # Summary
    # ========================================================================
    print("\n" + "=" * 60)
    print("✅ UMEP workflow complete!")
    print("=" * 60)
    print(f"📁 All outputs saved to: {output_folder_str}")


if __name__ == "__main__":
    main()
    gif_path = preview_pngs_to_gif(
        Path("output/umep_workflow"),
        pattern="shadow_*.preview.png",
        out_path="output/umep_workflow/shadow_preview.gif",
        duration_ms=500,
    )
    print(f"✅ GIF created: {gif_path}")
    gif_path = preview_pngs_to_gif(
        Path("output/umep_workflow"),
        pattern="tmrt_*.preview.png",
        out_path="output/umep_workflow/tmrt_preview.gif",
        duration_ms=500,
    )
    print(f"✅ GIF created: {gif_path}")
    gif_path = preview_pngs_to_gif(
        Path("output/umep_workflow/output_utci"),
        pattern="utci_*.preview.png",
        out_path="output/umep_workflow/utci_preview.gif",
        duration_ms=500,
    )
    print(f"✅ GIF created: {gif_path}")
