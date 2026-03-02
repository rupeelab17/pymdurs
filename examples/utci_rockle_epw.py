"""
Example: UTCI with Röckle wind field and pythermalcomfort (output GeoTIFF).

This example:
1. Computes wind speed per pixel for 10 directions (0°, 36°, ..., 324°) using
   the Röckle model (pymdurs.thermal.WindField).
2. Loads weather from an EPW file (Ta, RH, wind speed, wind direction).
3. Runs SOLWEIG to get mean radiant temperature (Tmrt) per timestep.
4. For each timestep, selects the Röckle wind raster for the EPW wind direction,
   scales it by EPW wind speed, and computes UTCI with pythermalcomfort using
   Tmrt, Ta, RH, and per-pixel wind speed.
5. Writes UTCI GeoTIFFs (one per timestep) and optionally a mean UTCI TIFF.

Prerequisites:
   - DEM and DSM (e.g. from umep_workflow or wind_field_from_ign).
   - Buildings (loaded from IGN in this script).
   - EPW file (e.g. la_rochelle_2025.epw).
   - solweig, pythermalcomfort, rasterio, numpy.

Usage:
   python examples/utci_rockle_epw.py

Output:
   ./output/utci_rockle/utci/utci_YYYYMMDD_HHMM.tif       (per timestep)
   ./output/utci_rockle/utci_mean.tif                     (mean over timesteps)
   ./output/utci_rockle/wind_speed_scaled/wind_speed_scaled_YYYYMMDD_HHMM.tif  (wind m/s scaled by EPW, per timestep)
   ./output/utci_rockle/wind_10dir/dir_XXX/rockle_zone.tif (Röckle zones: cavity, wake, etc.; when save_rockle_zone=True)
"""

from __future__ import annotations

import shutil
import sys
from datetime import datetime as dt
from pathlib import Path

import numpy as np
import rasterio

import pymdurs

# Optional: geopandas to convert bbox WGS84 → EPSG:2154 (same CRS as DEM/DSM)
try:
    import geopandas as gpd
    from shapely.geometry import box

    _has_gpd = True
except ImportError:
    _has_gpd = False

# Optional: matplotlib for UTCI plot with standard stress scale
try:
    import matplotlib.pyplot as plt
    from matplotlib.colors import LinearSegmentedColormap, Normalize
except ImportError:
    plt = None

# Optional: solweig for SurfaceData, Weather, calculate, and EPW/io
try:
    import solweig
    from solweig import io as solweig_io
except ImportError:
    solweig = None
    solweig_io = None

# pythermalcomfort for UTCI
try:
    from pythermalcomfort.models import utci as ptc_utci
except ImportError:
    ptc_utci = None


# 10 wind directions (degrees): 0, 36, ..., 324
# WIND_DIRECTIONS_DEG = [0, 36, 72, 108, 144, 180, 216, 252, 288, 324]
WIND_DIRECTIONS_DEG = [0, 180]
WIND_SPEED_REF_ROCKLE = (
    1.0  # m/s; Röckle rasters computed with this, then scaled by EPW ws
)


def _align_dem_to_dsm(dem_path: Path, dsm_path: Path, output_path: Path) -> Path:
    """Resample DEM to DSM grid. Returns path to aligned DEM."""
    with rasterio.open(dsm_path) as dsm_src:
        dsm_shape = (dsm_src.height, dsm_src.width)
        dsm_transform = dsm_src.transform
        dsm_crs = dsm_src.crs
    with rasterio.open(dem_path) as dem_src:
        out_array = np.empty(dsm_shape, dtype=dem_src.dtypes[0])
        from rasterio.warp import Resampling, reproject

        reproject(
            source=rasterio.band(dem_src, 1),
            destination=out_array,
            src_transform=dem_src.transform,
            src_crs=dem_src.crs,
            dst_transform=dsm_transform,
            dst_crs=dsm_crs,
            resampling=Resampling.bilinear,
        )
        nodata = dem_src.nodata
    with rasterio.open(
        output_path,
        "w",
        driver="GTiff",
        height=dsm_shape[0],
        width=dsm_shape[1],
        count=1,
        dtype=out_array.dtype,
        crs=dsm_crs,
        transform=dsm_transform,
        nodata=nodata,
    ) as out:
        out.write(out_array, 1)
    return output_path


def _load_weather_and_wind_direction(
    epw_path: str | Path,
    start: str,
    end: str,
) -> tuple[list, list[float]]:
    """Load weather list and wind direction per timestep from EPW (same filtering as Weather.from_epw)."""
    if solweig is None or solweig_io is None:
        raise ImportError("solweig is required. Install with: pip install solweig")
    path = Path(epw_path)
    if not path.exists():
        raise FileNotFoundError(f"EPW file not found: {path}")

    df, _ = solweig_io.read_epw(path)
    if df.empty:
        raise ValueError("EPW file contains no data")

    # Parse start/end (support "YYYY-MM-DD HH:MM" or "YYYY-MM-DD")
    def parse_dt(s: str) -> dt:
        if " " in s:
            return dt.strptime(s.strip(), "%Y-%m-%d %H:%M")
        return dt.strptime(s.strip(), "%Y-%m-%d")

    start_dt = parse_dt(start)
    end_dt = parse_dt(end)
    if end_dt.hour == 0 and end_dt.minute == 0:
        end_dt = end_dt.replace(hour=23, minute=59, second=59)

    # Filter by date range (solweig EPW index is naive datetime)
    mask = (df.index >= start_dt) & (df.index <= end_dt)
    df_filtered = df[mask]
    if df_filtered.empty:
        raise ValueError(
            f"No data in EPW for {start_dt} to {end_dt}. Check EPW file date range."
        )

    weather_list = []
    wind_directions = []
    for timestamp, row in df_filtered.iterrows():
        ts = (
            timestamp.to_pydatetime().replace(tzinfo=None)
            if hasattr(timestamp, "to_pydatetime")
            else timestamp
        )
        w = solweig.Weather(
            datetime=ts,
            ta=float(row["temp_air"]) if not np.isnan(row["temp_air"]) else 20.0,
            rh=float(row["relative_humidity"])
            if not np.isnan(row["relative_humidity"])
            else 50.0,
            global_rad=float(row["ghi"]) if not np.isnan(row["ghi"]) else 0.0,
            ws=float(row["wind_speed"]) if not np.isnan(row["wind_speed"]) else 1.0,
            pressure=(
                float(row["atmospheric_pressure"]) / 100.0
                if not np.isnan(row["atmospheric_pressure"])
                else 1013.25
            ),
            measured_direct_rad=float(row["dni"]) if not np.isnan(row["dni"]) else None,
            measured_diffuse_rad=float(row["dhi"])
            if not np.isnan(row["dhi"])
            else None,
        )
        weather_list.append(w)
        wd = (
            float(row["wind_direction"]) if not np.isnan(row["wind_direction"]) else 0.0
        )
        wind_directions.append(wd % 360.0)

    return weather_list, wind_directions


def _nearest_direction_index(wind_direction_deg: float) -> int:
    """Return index in WIND_DIRECTIONS_DEG nearest to wind_direction_deg (0-360)."""
    wd = wind_direction_deg % 360.0
    idx = min(
        range(len(WIND_DIRECTIONS_DEG)), key=lambda i: abs(WIND_DIRECTIONS_DEG[i] - wd)
    )
    return idx


def _compute_utci_raster(
    ta: float,
    rh: float,
    tmrt: np.ndarray,
    v_pixel: np.ndarray,
    nodata: float = np.nan,
) -> np.ndarray:
    """Compute UTCI per pixel with pythermalcomfort; invalid/NoData preserved."""
    if ptc_utci is None:
        raise ImportError(
            "pythermalcomfort is required. Install with: pip install pythermalcomfort"
        )

    valid = np.isfinite(tmrt) & np.isfinite(v_pixel) & (v_pixel >= 0)
    out = np.full_like(tmrt, nodata, dtype=np.float32)
    # Clamp v to avoid 0 (utci can be sensitive); use small minimum
    v_safe = np.where(valid, np.maximum(v_pixel, 0.01), 0.01)

    # pythermalcomfort.utci() returns a UTCI object: use .utci for the numeric value
    def _utci_value(res):
        v = getattr(res, "utci", res)
        return np.asarray(v, dtype=np.float64)

    try:
        res = ptc_utci(tdb=ta, tr=tmrt, v=v_safe, rh=rh, round_output=False)
        u = _utci_value(res)
        out[valid] = u.astype(np.float32)[valid]
    except Exception:
        # Fallback: scalar loop (extract .utci from return object)
        for i in range(tmrt.shape[0]):
            for j in range(tmrt.shape[1]):
                if valid[i, j]:
                    res = ptc_utci(
                        tdb=ta,
                        tr=float(tmrt[i, j]),
                        v=max(0.01, float(v_pixel[i, j])),
                        rh=rh,
                        round_output=False,
                    )
                    val = getattr(res, "utci", res)
                    out[i, j] = float(val)
    return out


def main() -> None:
    print("UTCI with Röckle wind and pythermalcomfort (GeoTIFF output)")
    print("=" * 60)

    if solweig is None:
        print("ERROR: solweig is required. pip install solweig")
        sys.exit(1)
    if ptc_utci is None:
        print("ERROR: pythermalcomfort is required. pip install pythermalcomfort")
        sys.exit(1)

    # Paths: same convention as umep_workflow_new (cwd-relative ./output/...)
    base = Path(__file__).resolve().parent.parent
    output_umep_cwd = Path("output/umep_workflow").resolve()
    output_umep_base = base / "output" / "umep_workflow"
    output_umep = output_umep_cwd if output_umep_cwd.exists() else output_umep_base
    output_folder = Path("output/utci_rockle").resolve()
    output_folder.mkdir(parents=True, exist_ok=True)
    output_folder_str = str(output_folder)

    # Where to find or create DEM/DSM (same as umep_workflow: ./output or ./output/umep_workflow)
    output_data_cwd = Path("output").resolve()
    output_data = (
        output_umep_base.parent if (base / "output").exists() else output_data_cwd
    )
    output_data.mkdir(parents=True, exist_ok=True)
    output_data_str = str(output_data)

    # Bbox (La Rochelle, same as umep_workflow_new)
    # WGS84 for DEM/Lidar IGN API; 2154 for Building + WindField (must match DEM/DSM CRS)
    min_x, min_y, max_x, max_y = -1.152223, 46.183282, -1.149637, 46.185459
    # min_x, min_y, max_x, max_y = -1.152704, 46.181627, -1.139893, 46.18699
    if _has_gpd:
        gdf_bbox = gpd.GeoDataFrame(
            geometry=[box(min_x, min_y, max_x, max_y)], crs="EPSG:4326"
        )
        gdf_bbox = gdf_bbox.to_crs(2154)
        min_x_2154, min_y_2154, max_x_2154, max_y_2154 = gdf_bbox.total_bounds
    else:
        min_x_2154, min_y_2154 = min_x, min_y
        max_x_2154, max_y_2154 = max_x, max_y
        print(
            "Note: install geopandas so bbox is converted to EPSG:2154; "
            "building/wind may be misaligned with DEM otherwise."
        )

    # DEM/DSM: 1) clipped from umep_workflow, 2) raw DEM/DSM from umep_workflow, 3) from output/, 4) create
    dem_path = dsm_path = cdsm_path = lc_path = None
    if (output_umep / "DEM_clip.tif").exists() and (
        output_umep / "DSM_clip.tif"
    ).exists():
        dem_path = output_umep / "DEM_clip.tif"
        dsm_path = output_umep / "DSM_clip.tif"
        cdsm_path = output_umep / "CDSM_clip.tif"
        lc_path = output_umep / "landcover_clip.tif"
        print(f"Using UMEP workflow rasters (clipped) from {output_umep}")
    elif (output_umep / "DEM.tif").exists() and (output_umep / "DSM.tif").exists():
        dem_path = output_umep / "DEM.tif"
        dsm_path = output_umep / "DSM.tif"
        cdsm_path = output_umep / "CDSM.tif"
        lc_path = (
            output_umep / "landcover_clip.tif"
            if (output_umep / "landcover_clip.tif").exists()
            else None
        )
        print(f"Using UMEP workflow rasters from {output_umep}")
    elif (output_data / "DEM.tif").exists() and (output_data / "DSM.tif").exists():
        dem_path = output_data / "DEM.tif"
        dsm_path = output_data / "DSM.tif"
        cdsm_path = output_data / "CDSM.tif"
        lc_path = (
            output_data / "landcover_clip.tif"
            if (output_data / "landcover_clip.tif").exists()
            else None
        )
        print(f"Using DEM/DSM from {output_data}")

    if dem_path is None or dsm_path is None:
        dem_path = dem_path or output_data / "DEM.tif"
        dsm_path = dsm_path or output_data / "DSM.tif"
        cdsm_path = cdsm_path or output_data / "CDSM.tif"
        lc_path = lc_path or output_data / "landcover_clip.tif"
        if not dem_path.exists():
            print("DEM not found. Downloading from IGN API (as in umep_workflow)...")
            dem = pymdurs.geometric.Dem(output_path=output_data_str)
            dem.set_bbox(min_x, min_y, max_x, max_y)
            dem.set_crs(2154)
            dem.run()
            print(f"  Saved {dem_path}")
        if not dsm_path.exists():
            print("DSM not found. Generating from LiDAR (as in umep_workflow)...")
            try:
                lidar = pymdurs.geometric.Lidar(output_path=output_data_str)
                lidar.set_bbox(min_x, min_y, max_x, max_y)
                lidar.set_crs(2154)
                lidar.run(file_name="DSM.tif", classification_list=[2, 6, 9])
                print(f"  Saved {dsm_path}")
            except (ValueError, Exception) as e:
                err = str(e)
                print(f"  Could not generate DSM: {err}")
                if "502" in err or "Bad Gateway" in err:
                    print(
                        "  (502 = IGN LiDAR service temporarily unavailable. Try again later.)"
                    )
                print(
                    "  Run first: python examples/umep_workflow_new.py  (then re-run this script),"
                )
                print(
                    "  or place DEM.tif and DSM.tif in output/ or output/umep_workflow/ and run again."
                )
                sys.exit(1)
        if dem_path.exists() and dsm_path.exists():
            print(f"Using DEM/DSM from {output_data}")

    # Align DEM to DSM if needed
    dem_for_wind = dem_path
    try:
        with rasterio.open(dem_path) as d, rasterio.open(dsm_path) as s:
            if (d.height, d.width) != (s.height, s.width):
                aligned = output_folder / "DEM_aligned.tif"
                dem_for_wind = _align_dem_to_dsm(dem_path, dsm_path, aligned)
                print(f"Aligned DEM to DSM grid: {dem_for_wind}")
    except Exception as e:
        print(f"Warning: could not align DEM to DSM: {e}")

    # Buildings: bbox WGS84 for IGN request; set_crs=2154 so stored coords match DEM/DSM (Röckle needs same CRS)
    print("\nLoading buildings from IGN...")
    buildings = pymdurs.geometric.Building(
        output_path=output_folder_str,
        defaultStoreyHeight=3.0,
        set_crs=2154,
    )
    buildings.set_bbox(min_x, min_y, max_x, max_y)
    buildings = buildings.run()
    print(f"Loaded {len(buildings)} buildings")

    # 1) Precompute 10 Röckle wind fields (bbox 2154 to match DEM)
    wind_10dir_dir = output_folder / "wind_10dir"
    wind_10dir_dir.mkdir(parents=True, exist_ok=True)

    # Without buildings in the domain, the wind field does not vary with direction (freestream only).
    if len(buildings) == 0:
        print(
            "  Warning: no buildings in bbox — wind rasters will be identical for all directions."
        )

    for d in WIND_DIRECTIONS_DEG:
        dir_sub = output_folder / "wind_10dir" / f"dir_{d:03d}"
        dir_sub.mkdir(parents=True, exist_ok=True)
        wind_dir_str = str(dir_sub)
        wind_per_dir = pymdurs.thermal.WindField(output_path=wind_dir_str)
        wind_per_dir.set_bbox(min_x_2154, min_y_2154, max_x_2154, max_y_2154)
        cfg = pymdurs.thermal.WindConfig(
            wind_speed_ref=WIND_SPEED_REF_ROCKLE,
            wind_direction=float(d),
            z_ref=10.0,
            profile_type="urban",
            resolution_m=1.0,
            use_mass_consistent_solver=True,
            solver_epsilon=0.01,
            solver_max_iter=500,
            output_height=1.5,
            solver_dx=1.0,
            solver_dy=1.0,
            solver_dz=1.0,
            save_rockle_zone=True,
        )
        wind_per_dir.run(cfg, str(dem_for_wind), str(dsm_path), buildings)
        src = dir_sub / "wind_speed.tif"
        dst = wind_10dir_dir / f"wind_speed_{d:03d}.tif"
        if src.exists():
            shutil.copy2(src, dst)
        print(
            f"  Röckle direction {d}° (wind_direction={cfg.wind_direction}) -> {dst.name}"
        )

    # 2) Load weather and wind direction from EPW
    epw_path = base / "examples" / "la_rochelle_2025.epw"
    if not epw_path.exists():
        epw_path = base / "la_rochelle_2025.epw"
    if not epw_path.exists():
        # When run from examples/, __file__ is examples/utci_rockle_epw.py -> base = project root
        epw_path = Path(__file__).resolve().parent / "la_rochelle_2025.epw"
    if not epw_path.exists():
        print("EPW not found. Place la_rochelle_2025.epw in examples/ or project root.")
        sys.exit(1)

    print("\nLoading weather from EPW...")
    weather_list, wind_directions = _load_weather_and_wind_direction(
        epw_path,
        start="2025-06-01 07:00",
        end="2025-06-01 19:00",
    )
    print(f"Loaded {len(weather_list)} timesteps")

    # 3) SOLWEIG: surface + Tmrt per timestep
    if lc_path is None or not lc_path.exists():
        lc_path = None
    print("\nPreparing surface and running SOLWEIG (Tmrt)...")
    surface = solweig.SurfaceData.prepare(
        dsm=str(dsm_path),
        working_dir=str(output_folder / "working"),
        cdsm=str(cdsm_path) if (cdsm_path and cdsm_path.exists()) else None,
        pixel_size=1.0,
        land_cover=str(lc_path) if lc_path and lc_path.exists() else None,
    )
    # Must use calculate_timeseries (not calculate) for a list of Weather + output_dir/outputs
    solweig.calculate(
        surface=surface,
        weather=weather_list,
        output_dir=str(output_folder),
        outputs=["tmrt"],
    )
    tmrt_dir = output_folder / "tmrt"
    print(f"Tmrt GeoTIFFs in {tmrt_dir}")

    # 4) For each timestep: select wind raster, scale, compute UTCI, write TIFF
    utci_dir = output_folder / "utci"
    utci_dir.mkdir(parents=True, exist_ok=True)
    wind_scaled_dir = output_folder / "wind_speed_scaled"
    wind_scaled_dir.mkdir(parents=True, exist_ok=True)
    print(f"Wind speed scaled (m/s, EPW) GeoTIFFs in {wind_scaled_dir}")

    with rasterio.open(dsm_path) as dsm_ref:
        profile = dsm_ref.profile.copy()
        height, width = dsm_ref.height, dsm_ref.width

    profile.update(dtype=rasterio.float32, count=1, nodata=np.nan)
    utci_accum = np.zeros((height, width), dtype=np.float64)
    utci_count = np.zeros((height, width), dtype=np.float64)

    for i, (weather, wdir_deg) in enumerate(
        zip(weather_list, wind_directions, strict=True)
    ):
        ts_str = weather.datetime.strftime("%Y%m%d_%H%M")
        tmrt_path = tmrt_dir / f"tmrt_{ts_str}.tif"
        if not tmrt_path.exists():
            print(f"  Skip {ts_str}: no Tmrt file")
            continue

        idx = _nearest_direction_index(wdir_deg)
        print(
            f"  {ts_str} EPW wdir={wdir_deg:.0f}° -> raster {WIND_DIRECTIONS_DEG[idx]:.0f}°"
        )
        dir_used = WIND_DIRECTIONS_DEG[idx]
        wind_path = wind_10dir_dir / f"wind_speed_{dir_used:03d}.tif"
        with rasterio.open(tmrt_path) as src:
            tmrt = src.read(1)
        with rasterio.open(wind_path) as src:
            wind_speed_raster = src.read(1)

        # Align wind to Tmrt/DSM grid if SOLWEIG output resolution differs
        if tmrt.shape != (height, width) or wind_speed_raster.shape != (height, width):
            from rasterio.warp import Resampling, reproject

            if wind_speed_raster.shape != (height, width):
                wind_resampled = np.empty(
                    (height, width), dtype=wind_speed_raster.dtype
                )
                with rasterio.open(wind_path) as wsrc:
                    reproject(
                        source=rasterio.band(wsrc, 1),
                        destination=wind_resampled,
                        src_transform=wsrc.transform,
                        src_crs=wsrc.crs,
                        dst_transform=profile["transform"],
                        dst_crs=profile["crs"],
                        resampling=Resampling.bilinear,
                    )
                wind_speed_raster = wind_resampled
            if tmrt.shape != (height, width):
                tmrt_resampled = np.empty((height, width), dtype=tmrt.dtype)
                with rasterio.open(tmrt_path) as tsrc:
                    reproject(
                        source=rasterio.band(tsrc, 1),
                        destination=tmrt_resampled,
                        src_transform=tsrc.transform,
                        src_crs=tsrc.crs,
                        dst_transform=profile["transform"],
                        dst_crs=profile["crs"],
                        resampling=Resampling.bilinear,
                    )
                tmrt = tmrt_resampled

        # Scale Röckle wind by EPW wind speed (Röckle was computed with WIND_SPEED_REF_ROCKLE)
        ws_epw = max(weather.ws, 0.01)
        v_pixel = wind_speed_raster * (ws_epw / WIND_SPEED_REF_ROCKLE)

        # Write wind_speed_scaled per timestep (m/s actually used for UTCI)
        wind_scaled_path = wind_scaled_dir / f"wind_speed_scaled_{ts_str}.tif"
        with rasterio.open(wind_scaled_path, "w", **profile) as dst:
            dst.write(v_pixel.astype(np.float32), 1)

        utci_grid = _compute_utci_raster(
            weather.ta,
            weather.rh,
            tmrt,
            v_pixel,
            nodata=np.nan,
        )
        utci_accum += np.where(np.isfinite(utci_grid), utci_grid, 0.0)
        utci_count += np.isfinite(utci_grid).astype(np.float64)

        out_path = utci_dir / f"utci_{ts_str}.tif"
        with rasterio.open(out_path, "w", **profile) as dst:
            dst.write(utci_grid.astype(np.float32), 1)
        print(
            f"  {ts_str} EPW wdir={wdir_deg:.0f}° -> raster {dir_used}° -> {out_path.name}"
        )

    # Mean UTCI
    mean_utci = np.full((height, width), np.nan, dtype=np.float32)
    valid = utci_count > 0
    mean_utci[valid] = (utci_accum[valid] / utci_count[valid]).astype(np.float32)
    mean_path = output_folder / "utci_mean.tif"
    with rasterio.open(mean_path, "w", **profile) as dst:
        dst.write(mean_utci, 1)
    print(f"\nMean UTCI written: {mean_path}")

    # Plot utci_mean with standard UTCI stress scale (Blazejczyk et al.)
    if plt is not None:
        boundaries = [-40, -27, -13, 0, 9, 26, 32, 38, 46]
        colors = [
            "#00007f",
            "#0301c1",
            "#0000fb",
            "#0061fe",
            "#01c0fd",
            "#00c000",
            "#ff6601",
            "#ff3200",
            "#cc0001",
            "#7e0305",
        ]
        cmap_utci = LinearSegmentedColormap.from_list(
            "utci_stress", colors, N=len(boundaries) - 1
        )
        norm_utci = Normalize(vmin=min(boundaries), vmax=max(boundaries))
        fig, ax = plt.subplots(figsize=(8, 8))
        ax.set_xticks([])
        ax.set_yticks([])
        ax.set_title("UTCI (Universal Thermal Climate Index) — mean")
        tr = profile.get("transform")
        if tr is not None:
            ext = (tr.c, tr.c + width * tr.a, tr.f + height * tr.e, tr.f)
            im = ax.imshow(
                mean_utci,
                cmap=cmap_utci,
                norm=norm_utci,
                extent=ext,
                origin="upper",
                interpolation="nearest",
            )
        else:
            im = ax.imshow(
                mean_utci,
                cmap=cmap_utci,
                norm=norm_utci,
                origin="upper",
                interpolation="nearest",
            )
        plt.colorbar(im, ax=ax, label="UTCI (°C)")
        plot_path = output_folder / "utci_mean.png"
        fig.savefig(plot_path, dpi=150, bbox_inches="tight")
        plt.close(fig)
        print(f"UTCI mean plot saved: {plot_path}")
    print("Done.")


if __name__ == "__main__":
    main()
