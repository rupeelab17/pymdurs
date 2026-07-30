"""
Example: UTCI with QES-Winds and pythermalcomfort (output GeoTIFF).

This example:
1. Computes wind speed per pixel for 10 directions (0°, 36°, ..., 324°) using
   QES-Winds (pyQES.pywinds), with reference speed 1 m/s. Buildings and LiDAR
   tree crowns (vegetationParams) are included.
2. Loads weather from an EPW file (Ta, RH, wind speed, wind direction).
3. Runs SOLWEIG to get mean radiant temperature (Tmrt) per timestep.
4. For each timestep, selects the QES wind raster for the EPW wind direction,
   scales it by EPW wind speed, and computes UTCI with pythermalcomfort using
   Tmrt, Ta, RH, and per-pixel wind speed.
5. Writes UTCI GeoTIFFs (one per timestep) and a mean UTCI TIFF.

Inspired by:
   - examples/utci_rockle_epw.py (UTCI + EPW + SOLWEIG pipeline)
   - pyQES examples/pymdurs_workflow/run_from_bbox.py (QES-Winds + trees)

Prerequisites:
   - DEM and DSM (e.g. from umep_workflow or wind_field_from_ign).
   - Buildings and trees (loaded from IGN / LiDAR in this script).
   - EPW file (e.g. la_rochelle_2025.epw).
   - solweig, pythermalcomfort, rasterio, numpy, geopandas.
   - pyQES with geo/io extras: ``uv pip install "pyQES[geo,io]"``

Usage:
   python examples/utci_qes_epw.py

Output:
   ./output/utci_qes/utci/utci_YYYYMMDD_HHMM.tif
   ./output/utci_qes/utci_mean.tif
   ./output/utci_qes/wind_speed_scaled/wind_speed_scaled_YYYYMMDD_HHMM.tif
   ./output/utci_qes/wind_10dir/wind_speed_XXX.tif
   ./output/utci_qes/trees.shp  (QES crown polygons)
"""

from __future__ import annotations

import shutil
import sys
from datetime import datetime as dt
from pathlib import Path

import numpy as np
import rasterio

import pymdurs
from pymdurs.vegetation import extract_tree_crowns

try:
    import geopandas as gpd

    _has_gpd = True
except ImportError:
    _has_gpd = False

try:
    import matplotlib.pyplot as plt
    from matplotlib.colors import LinearSegmentedColormap, Normalize
except ImportError:
    plt = None

try:
    import solweig
    from solweig import io as solweig_io
except ImportError:
    solweig = None
    solweig_io = None

try:
    from pythermalcomfort.models import utci as ptc_utci
except ImportError:
    ptc_utci = None

try:
    from pyQES import pywinds
    from pyQES.util import geo
    from pyQES.util.config import (
        SensorParameters,
        TimeSeries,
        VegetationParameters,
        WindsParameters,
    )
except ImportError:
    pywinds = None
    geo = None
    SensorParameters = None  # type: ignore[misc, assignment]
    TimeSeries = None  # type: ignore[misc, assignment]
    VegetationParameters = None  # type: ignore[misc, assignment]
    WindsParameters = None  # type: ignore[misc, assignment]


WORKING_CRS = 2154
MAX_DOMAIN_CELLS = 6_000_000
WIND_DIRECTIONS_DEG = [0, 36, 72, 108, 144, 180, 216, 252, 288, 324]
WIND_SPEED_REF = 1.0  # m/s; QES rasters computed with this, then scaled by EPW ws
CELL_SIZE = (2.5, 2.5, 1.5)
HALO_X = 40.0
HALO_Y = 40.0
TIF_Z_AGL = 1.5
SENSOR_HEIGHT = 10.0

TREES_POINTS_NAME = "trees_points.shp"
TREES_QES_NAME = "trees.shp"
TREES_LAYER = "trees"
_TREE_REQUIRED_FIELDS = ("H", "D", "LAI")
LAI = 4.0
MIN_TREE_HEIGHT = 2.0
MAX_TREE_HEIGHT = 30.0
TREES_MIN_SPACING = 3.0
# Isolated-tree wake (~11×H) blanks near-ground wind with dense crowns.
TREE_WAKE = False

_HEIGHT_CANDIDATES = (
    "hauteur",
    "height",
    "HAUTEUR",
    "hauteur_mean",
    "H_MEDIANE",
    "H_MAX",
)


def _ensure_hauteur(gdf: gpd.GeoDataFrame) -> gpd.GeoDataFrame:
    """Ensure a ``hauteur`` column exists for QES building preprocessing."""
    if "hauteur" in gdf.columns:
        return gdf
    for name in _HEIGHT_CANDIDATES:
        if name in gdf.columns:
            gdf = gdf.copy()
            gdf["hauteur"] = gdf[name]
            return gdf
    raise SystemExit(
        "No building height attribute found. Expected one of: "
        + ", ".join(_HEIGHT_CANDIDATES)
        + f". Got columns: {list(gdf.columns)}"
    )


def project_dem_to_meters(dem_path: Path, epsg: int = WORKING_CRS) -> Path:
    """Reproject DEM to a metric CRS so QES domain sizing uses metres."""
    from rasterio.warp import Resampling, calculate_default_transform, reproject

    out_path = dem_path.with_name(f"DEM_{epsg}.tif")
    with rasterio.open(dem_path) as src:
        src_epsg = src.crs.to_epsg() if src.crs is not None else None
        if src.crs is not None and not src.crs.is_geographic and src_epsg == epsg:
            return dem_path

        dst_crs = f"EPSG:{epsg}"
        transform, width, height = calculate_default_transform(
            src.crs, dst_crs, src.width, src.height, *src.bounds
        )
        profile = src.meta.copy()
        profile.update(crs=dst_crs, transform=transform, width=width, height=height)

        with rasterio.open(out_path, "w", **profile) as dst:
            for band_idx in range(1, src.count + 1):
                reproject(
                    source=rasterio.band(src, band_idx),
                    destination=rasterio.band(dst, band_idx),
                    src_transform=src.transform,
                    src_crs=src.crs,
                    dst_transform=transform,
                    dst_crs=dst_crs,
                    resampling=Resampling.bilinear,
                )
    return out_path


def clip_dem_to_mask(dem_path: Path, mask_shp: Path, out_path: Path) -> Path:
    """Clip a projected DEM to the mask polygon (smaller QES domain)."""
    from rasterio.mask import mask as rio_mask

    mask_gdf = gpd.read_file(mask_shp)
    with rasterio.open(dem_path) as src:
        if mask_gdf.crs is not None and src.crs is not None:
            mask_gdf = mask_gdf.to_crs(src.crs)
        data, transform = rio_mask(src, mask_gdf.geometry, crop=True, filled=True)
        profile = src.profile.copy()
        profile.update(
            height=data.shape[1],
            width=data.shape[2],
            transform=transform,
        )
        with rasterio.open(out_path, "w", **profile) as dst:
            dst.write(data)
    return out_path


def fetch_buildings(
    work_dir: Path, bbox: tuple[float, float, float, float]
) -> Path:
    """Download buildings from IGN and write ``buildings.shp`` (EPSG:2154)."""
    buildings = pymdurs.geometric.Building(
        output_path=str(work_dir),
        defaultStoreyHeight=3.0,
    )
    buildings.set_bbox(*bbox)
    buildings.set_crs(WORKING_CRS)
    buildings = buildings.run()

    geojson = buildings.get_geojson()
    features = geojson.get("features", []) if geojson else []
    if not features:
        raise SystemExit("No buildings returned for this bbox.")

    gdf = gpd.GeoDataFrame.from_features(features, crs="EPSG:4326")
    gdf = gdf.to_crs(epsg=WORKING_CRS)
    gdf = _ensure_hauteur(gdf)

    out_shp = work_dir / "buildings.shp"
    gdf.to_file(out_shp, driver="ESRI Shapefile")
    return out_shp


def trees_from_cdsm(
    cdsm_path: Path,
    out_points: Path,
    *,
    lai: float = LAI,
    min_tree_height: float = MIN_TREE_HEIGHT,
    min_distance: int = 5,
) -> Path:
    """Extract tree tops from an existing CDSM/CHM GeoTIFF (no LiDAR re-fetch)."""
    with rasterio.open(cdsm_path) as src:
        # Single-band CDSM, or multi-band LiDAR stack (band 3 = CHM).
        band = 3 if src.count >= 3 else 1
        chm = src.read(band).astype(np.float64)
        nodata = src.nodata
        if nodata is not None:
            chm = np.where(chm == nodata, np.nan, chm)
        transform = src.transform
        crs = src.crs.to_string() if src.crs is not None else f"EPSG:{WORKING_CRS}"

    gdf = extract_tree_crowns(
        chm,
        transform,
        min_tree_height=min_tree_height,
        min_distance=min_distance,
        lai=lai,
        crs=crs,
    )
    out_points.parent.mkdir(parents=True, exist_ok=True)
    gdf.to_file(out_points, driver="ESRI Shapefile")
    print(f"trees (pts from CDSM): {out_points}  ({len(gdf)} tops)")
    return out_points


def _thin_trees_by_spacing(
    gdf: gpd.GeoDataFrame, min_spacing: float
) -> gpd.GeoDataFrame:
    """Keep tallest trees first; drop any within ``min_spacing`` of a kept tree."""
    if min_spacing <= 0 or gdf.empty:
        return gdf
    ordered = gdf.sort_values("H", ascending=False)
    kept_idx: list[object] = []
    kept_pts: list[tuple[float, float]] = []
    spacing2 = float(min_spacing) ** 2
    for idx, row in ordered.iterrows():
        geom = row.geometry
        if geom is None or geom.is_empty:
            continue
        c = geom.centroid
        x, y = float(c.x), float(c.y)
        if any((x - kx) ** 2 + (y - ky) ** 2 < spacing2 for kx, ky in kept_pts):
            continue
        kept_idx.append(idx)
        kept_pts.append((x, y))
    out = gdf.loc[kept_idx].copy()
    print(
        f"Thinned trees: {len(gdf)} → {len(out)} "
        f"(min spacing {min_spacing:g} m, prefer taller)."
    )
    return out


def prepare_trees_shp(
    points_shp: Path,
    mask_shp: Path,
    out_shp: Path,
    *,
    max_tree_height: float | None = MAX_TREE_HEIGHT,
    min_spacing: float = TREES_MIN_SPACING,
) -> Path | None:
    """Convert point tops to crown polygons, clip to mask, write ``trees.shp``.

    QES only keeps polygon geometries; crowns are circles of radius ``D/2``.
    Returns ``None`` if no trees remain after clipping/thinning.
    """
    gdf = gpd.read_file(points_shp)
    missing = [f for f in _TREE_REQUIRED_FIELDS if f not in gdf.columns]
    if missing:
        raise SystemExit(
            f"Tree shapefile missing fields {missing}. "
            f"Got columns: {list(gdf.columns)}"
        )

    n_in = len(gdf)
    if max_tree_height is not None:
        gdf = gdf[gdf["H"] <= float(max_tree_height)].copy()
        n_drop = n_in - len(gdf)
        if n_drop:
            print(
                f"Warning: dropped {n_drop} trees with H > {max_tree_height} m "
                "(CHM outliers)."
            )

    mask_gdf = gpd.read_file(mask_shp)
    if gdf.crs is None:
        gdf = gdf.set_crs(epsg=WORKING_CRS)
    if mask_gdf.crs is not None and gdf.crs != mask_gdf.crs:
        gdf = gdf.to_crs(mask_gdf.crs)

    if not gdf.empty:
        geom_types = set(gdf.geometry.geom_type.unique())
        if "Point" in geom_types or "MultiPoint" in geom_types:
            gdf = gdf.copy()
            gdf["geometry"] = gdf.apply(
                lambda row: (
                    row.geometry.buffer(float(row["D"]) / 2.0)
                    if row.geometry is not None
                    and row.geometry.geom_type in ("Point", "MultiPoint")
                    else row.geometry
                ),
                axis=1,
            )

    mask_union = mask_gdf.geometry.union_all()
    if not gdf.empty:
        gdf = gdf[gdf.geometry.intersects(mask_union)].copy()
    if gdf.empty:
        print("Warning: no trees inside mask after clip; omitting vegetation.")
        return None

    gdf = _thin_trees_by_spacing(gdf, min_spacing)
    if gdf.empty:
        print("Warning: no trees left after thinning; omitting vegetation.")
        return None

    gdf = gdf[list(_TREE_REQUIRED_FIELDS) + ["geometry"]]
    gdf.to_file(out_shp, driver="ESRI Shapefile")
    print(f"trees (QES): {out_shp}  ({len(gdf)} crowns)")
    return out_shp


def resolve_trees(
    work_dir: Path,
    mask_shp: Path,
    cdsm_path: Path | None,
) -> Path | None:
    """Return prepared ``trees.shp`` from existing points or CDSM (no LiDAR re-fetch)."""
    trees_shp = work_dir / TREES_QES_NAME
    trees_points = work_dir / TREES_POINTS_NAME

    if trees_points.is_file() or trees_shp.is_file():
        src = trees_points if trees_points.is_file() else trees_shp
        print(f"Preparing trees from existing {src.name}...")
        return prepare_trees_shp(
            src,
            mask_shp,
            trees_shp,
            max_tree_height=MAX_TREE_HEIGHT,
            min_spacing=TREES_MIN_SPACING,
        )

    if cdsm_path is None or not Path(cdsm_path).is_file():
        print("Warning: no CDSM — omitting vegetation (run umep_workflow for CDSM).")
        return None

    print(f"Extracting trees from CDSM: {cdsm_path}")
    points_shp = trees_from_cdsm(
        Path(cdsm_path),
        trees_points,
        lai=LAI,
        min_tree_height=MIN_TREE_HEIGHT,
    )
    return prepare_trees_shp(
        points_shp,
        mask_shp,
        trees_shp,
        max_tree_height=MAX_TREE_HEIGHT,
        min_spacing=TREES_MIN_SPACING,
    )


def check_domain_size(
    params: object,
    dem: Path,
    buildings: Path,
    *,
    trees: Path | None = None,
    force: bool = False,
) -> tuple[int, int, int]:
    """Print domain size and abort if the mesh is dangerously large."""
    if geo is None:
        raise ImportError('pyQES is required. Install with: uv pip install "pyQES[geo,io]"')
    domain = geo.compute_domain_cells(params, dem, buildings, trees_shp=trees)
    n_cells = domain[0] * domain[1] * domain[2]
    print(f"domain:       {domain[0]} x {domain[1]} x {domain[2]}  ({n_cells:,} cells)")
    if n_cells > MAX_DOMAIN_CELLS and not force:
        raise SystemExit(
            f"Domain too large ({n_cells:,} cells > {MAX_DOMAIN_CELLS:,}). "
            "Increase cell size, shrink bbox, or set force=True (may segfault)."
        )
    return domain


def run_qes_wind(
    *,
    dem: Path,
    buildings_src: Path,
    buildings_mask: Path,
    work_dir: Path,
    direction_deg: float,
    speed: float = WIND_SPEED_REF,
    trees_shp: Path | None = None,
) -> Path:
    """Run QES-Winds for one direction and export |V| GeoTIFF at TIF_Z_AGL."""
    if (
        pywinds is None
        or WindsParameters is None
        or SensorParameters is None
        or TimeSeries is None
    ):
        raise ImportError('pyQES is required. Install with: uv pip install "pyQES[geo,io]"')

    params = WindsParameters()
    params.simulation_parameters.dem = str(dem.resolve())
    params.simulation_parameters.cell_size = CELL_SIZE
    params.simulation_parameters.halo_x = HALO_X
    params.simulation_parameters.halo_y = HALO_Y
    params.simulation_parameters.domain_rotation = 0.0
    # Street canyon can segfault in native QES on some IGN building sets.
    params.simulation_parameters.street_canyon_flag = 0
    params.buildings_params.street_canyon_flag = 0
    params.buildings_params.shp_height_field = "hauteur"
    params.buildings_params.shp_building_layer = "buildings_clipped"

    if trees_shp is not None:
        if VegetationParameters is None:
            raise ImportError(
                'pyQES VegetationParameters required. '
                'Install with: uv pip install "pyQES[geo,io]"'
            )
        params.vegetation_params = VegetationParameters(
            wake_flag=1 if TREE_WAKE else 0,
            shp_file=str(trees_shp.resolve()),
            shp_tree_layer=TREES_LAYER,
        )

    check_domain_size(params, dem, buildings_src, trees=trees_shp)

    sensor = SensorParameters(
        time_series=[
            TimeSeries(
                speed=speed,
                direction=direction_deg,
                height=SENSOR_HEIGHT,
                site_z0=0.24,
            )
        ]
    )

    pywinds.run(
        config=params,
        sensor=sensor,
        solver="cpu",
        work_dir=work_dir,
        auto_preprocess=True,
        workspace=False,
        buildings_src=buildings_src,
        buildings_mask=buildings_mask,
    )
    tif = pywinds.to_tif(z=TIF_Z_AGL, verbose=False, mask_buildings=False)
    return Path(tif)


def _load_weather_and_wind_direction(
    epw_path: str | Path,
    start: str,
    end: str,
) -> tuple[list, list[float]]:
    """Load weather list and wind direction per timestep from EPW."""
    if solweig is None or solweig_io is None:
        raise ImportError("solweig is required. Install with: pip install solweig")
    path = Path(epw_path)
    if not path.exists():
        raise FileNotFoundError(f"EPW file not found: {path}")

    df, _ = solweig_io.read_epw(path)
    if df.empty:
        raise ValueError("EPW file contains no data")

    def parse_dt(s: str) -> dt:
        if " " in s:
            return dt.strptime(s.strip(), "%Y-%m-%d %H:%M")
        return dt.strptime(s.strip(), "%Y-%m-%d")

    start_dt = parse_dt(start)
    end_dt = parse_dt(end)
    if end_dt.hour == 0 and end_dt.minute == 0:
        end_dt = end_dt.replace(hour=23, minute=59, second=59)

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
    return min(
        range(len(WIND_DIRECTIONS_DEG)),
        key=lambda i: abs(WIND_DIRECTIONS_DEG[i] - wd),
    )


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
    v_safe = np.where(valid, np.maximum(v_pixel, 0.01), 0.01)

    def _utci_value(res):
        v = getattr(res, "utci", res)
        return np.asarray(v, dtype=np.float64)

    try:
        res = ptc_utci(tdb=ta, tr=tmrt, v=v_safe, rh=rh, round_output=False)
        u = _utci_value(res)
        out[valid] = u.astype(np.float32)[valid]
    except Exception:
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


_COSIA_TO_UMEP = {
    "Bâtiment": 2,
    "Zone imperméable": 1,
    "Zone perméable": 6,
    "Piscine": 7,
    "Serre": 1,
    "Sol nu": 6,
    "Surface eau": 7,
    "Neige": 7,
    "Conifère": 3,
    "Feuillu": 4,
    "Coupe": 5,
    "Broussaille": 5,
    "Pelouse": 5,
    "Culture": 5,
    "Terre labourée": 6,
    "Vigne": 5,
    "Autre": 1,
}


def _generate_landcover(
    bbox_wgs84: tuple[float, float, float, float],
    output_data: Path,
    working_crs: int = WORKING_CRS,
) -> Path:
    """Download COSIA from IGN and rasterize to a UMEP landcover GeoTIFF."""
    from rasterio.features import rasterize, shapes
    from rasterio.transform import from_bounds
    from shapely.geometry import shape

    import geopandas as gpd
    from pymdurs.geometric_helpers import TABLE_COLOR_COSIA

    def _hex_to_rgb(hex_color: str) -> tuple[int, int, int]:
        h = hex_color.lstrip("#")
        return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)

    lc_path = output_data / "landcover_clip.tif"
    if lc_path.exists():
        return lc_path

    print("Landcover not found. Downloading COSIA from IGN API...")
    cosia = pymdurs.geometric.Cosia(output_path=str(output_data))
    cosia.set_bbox(*bbox_wgs84)
    cosia.set_crs(working_crs)
    cosia = cosia.run_ign()
    cosia_tiff = cosia.get_path_save_tiff()
    print(f"  COSIA downloaded: {cosia_tiff}")

    rgb_to_class: dict[tuple[int, int, int], str] = {
        _hex_to_rgb(color): classe for classe, color in TABLE_COLOR_COSIA.items()
    }

    with rasterio.open(cosia_tiff) as src:
        image = src.read()
        transform = src.transform
        crs = src.crs
        combined = (
            (image[0].astype(np.uint32) << 16)
            + (image[1].astype(np.uint32) << 8)
            + image[2].astype(np.uint32)
        )
        results = list(shapes(combined, transform=transform))

    geoms, rgb_values = [], []
    for geom, value in results:
        v = int(value)
        rgb_values.append(((v >> 16) & 255, (v >> 8) & 255, v & 255))
        geoms.append(shape(geom))

    gdf = gpd.GeoDataFrame({"rgb": rgb_values, "geometry": geoms}, crs=crs)
    gdf["classe"] = gdf["rgb"].apply(
        lambda rgb: rgb_to_class.get(
            min(rgb_to_class, key=lambda t: sum((a - b) ** 2 for a, b in zip(rgb, t))),
            "Autre",
        )
    )
    gdf["type"] = gdf["classe"].map(_COSIA_TO_UMEP).fillna(1).astype(int)
    gdf = gdf[gdf.geometry.notna()].to_crs(working_crs)

    bounds = gdf.total_bounds
    resolution = 0.5
    width = max(1, int((bounds[2] - bounds[0]) / resolution))
    height = max(1, int((bounds[3] - bounds[1]) / resolution))
    t = from_bounds(bounds[0], bounds[1], bounds[2], bounds[3], width, height)

    raster = rasterize(
        shapes=((geom, val) for geom, val in zip(gdf.geometry, gdf["type"])),
        out_shape=(height, width),
        transform=t,
        fill=0,
        dtype=np.uint8,
        all_touched=False,
    )

    with rasterio.open(
        lc_path, "w",
        driver="GTiff", height=height, width=width,
        count=1, dtype=raster.dtype,
        crs=gdf.crs, transform=t,
        compress="lzw", nodata=0,
    ) as dst:
        dst.write(raster, 1)

    print(f"  Landcover saved: {lc_path}")
    return lc_path


def _fetch_dem(bbox_wgs84: tuple[float, float, float, float], output_data: Path) -> Path:
    """Download DEM from IGN into ``output_data/DEM.tif``."""
    dem_path = output_data / "DEM.tif"
    if dem_path.exists():
        return dem_path
    print("DEM not found. Downloading from IGN API...")
    dem = pymdurs.geometric.Dem(output_path=str(output_data))
    dem.set_bbox(*bbox_wgs84)
    dem.set_crs(WORKING_CRS)
    dem.run()
    print(f"  Saved {dem_path}")
    return dem_path


def _fetch_dsm_cdsm(
    bbox_wgs84: tuple[float, float, float, float],
    output_data: Path,
) -> tuple[Path, Path]:
    """Generate DSM/CDSM from LiDAR into ``output_data``."""
    dsm_path = output_data / "DSM.tif"
    cdsm_path = output_data / "CDSM.tif"
    if dsm_path.exists():
        return dsm_path, cdsm_path
    print("DSM not found. Generating from LiDAR...")
    try:
        lidar = pymdurs.geometric.Lidar(output_path=str(output_data))
        lidar.set_bbox(*bbox_wgs84)
        lidar.set_crs(WORKING_CRS)
        lidar.run(file_name="DSM.tif", classification_list=[2, 6, 9])
        print(f"  Saved {dsm_path}")
        if not cdsm_path.exists():
            lidar.run(file_name="CDSM.tif", classification_list=[3, 4, 5])
            print(f"  Saved {cdsm_path}")
    except (ValueError, Exception) as e:
        print(f"  Could not generate DSM: {e}")
        print(
            "  Run first: python examples/umep_workflow_new.py, "
            "or place DEM.tif and DSM.tif in output/."
        )
        sys.exit(1)
    return dsm_path, cdsm_path


def _resolve_rasters(
    output_umep: Path,
    output_data: Path,
    bbox_wgs84: tuple[float, float, float, float],
) -> tuple[Path, Path, Path | None, Path | None]:
    """Resolve DEM/DSM/CDSM/landcover paths, fetching if missing."""
    candidates = (
        (output_umep, "DEM_clip.tif", "DSM_clip.tif", "CDSM_clip.tif"),
        (output_umep, "DEM.tif", "DSM.tif", "CDSM.tif"),
        (output_data, "DEM.tif", "DSM.tif", "CDSM.tif"),
    )

    dem_path = dsm_path = cdsm_path = None
    for folder, dem_name, dsm_name, cdsm_name in candidates:
        dem, dsm = folder / dem_name, folder / dsm_name
        if dem.exists() and dsm.exists():
            dem_path, dsm_path, cdsm_path = dem, dsm, folder / cdsm_name
            print(f"Using DEM/DSM from {folder}")
            break
    else:
        dem_path = _fetch_dem(bbox_wgs84, output_data)
        dsm_path, cdsm_path = _fetch_dsm_cdsm(bbox_wgs84, output_data)

    for lc_candidate in (output_umep / "landcover_clip.tif", output_data / "landcover_clip.tif"):
        if lc_candidate.exists():
            return dem_path, dsm_path, cdsm_path, lc_candidate

    return dem_path, dsm_path, cdsm_path, _generate_landcover(bbox_wgs84, output_data)


def _prepare_qes_dem(
    dem_path: Path,
    output_folder: Path,
    bbox_wgs84: tuple[float, float, float, float],
) -> tuple[Path, Path]:
    """Project/clip DEM and ensure mask.shp for QES."""
    mask_shp = output_folder / "mask.shp"
    dem_clip = output_folder / "DEM_clip.tif"

    umep_mask = Path("output/umep_workflow").resolve() / "mask.shp"
    if not mask_shp.exists():
        if umep_mask.exists():
            for ext in (".shp", ".shx", ".dbf", ".prj", ".cpg"):
                src = umep_mask.with_suffix(ext)
                if src.exists():
                    shutil.copy2(src, mask_shp.with_suffix(ext))
        else:
            print("Fetching DEM + mask from IGN for QES...")
            dem = pymdurs.geometric.Dem(output_path=str(output_folder))
            dem.set_bbox(*bbox_wgs84)
            dem.set_crs(WORKING_CRS)
            dem = dem.run()
            mask_from_dem = Path(dem.get_path_save_mask())
            if mask_from_dem.is_file() and mask_from_dem != mask_shp:
                for ext in (".shp", ".shx", ".dbf", ".prj", ".cpg"):
                    src = mask_from_dem.with_suffix(ext)
                    if src.exists():
                        shutil.copy2(src, mask_shp.with_suffix(ext))

    dem_m = project_dem_to_meters(dem_path)
    if mask_shp.exists():
        dem_clip = clip_dem_to_mask(dem_m, mask_shp, dem_clip)
    else:
        dem_clip = dem_m
        print("Warning: no mask.shp — using unclipped projected DEM for QES.")

    return dem_clip, mask_shp


def main() -> None:
    print("UTCI with QES-Winds and pythermalcomfort (GeoTIFF output)")
    print("=" * 60)

    if solweig is None:
        print("ERROR: solweig is required. pip install solweig")
        sys.exit(1)
    if ptc_utci is None:
        print("ERROR: pythermalcomfort is required. pip install pythermalcomfort")
        sys.exit(1)
    if pywinds is None:
        print(
            'ERROR: pyQES is required. Install with: '
            'uv pip install "pyQES[geo,io]"'
        )
        sys.exit(1)
    if not _has_gpd:
        print("ERROR: geopandas is required for QES building/DEM prep.")
        sys.exit(1)

    base = Path(__file__).resolve().parent.parent
    output_umep_cwd = Path("output/umep_workflow").resolve()
    output_umep_base = base / "output" / "umep_workflow"
    output_umep = output_umep_cwd if output_umep_cwd.exists() else output_umep_base
    output_folder = Path("output/utci_qes").resolve()
    output_folder.mkdir(parents=True, exist_ok=True)

    output_data_cwd = Path("output").resolve()
    output_data = (
        output_umep_base.parent if (base / "output").exists() else output_data_cwd
    )
    output_data.mkdir(parents=True, exist_ok=True)

    # Small La Rochelle bbox (safe for QES domain size)
    bbox_wgs84 = (-1.152704,46.181627,-1.139893,46.18699)

    print(f"bbox (WGS84): {bbox_wgs84}")
    print(f"cell_size:    {CELL_SIZE}")
    print(f"output:       {output_folder}")

    dem_path, dsm_path, cdsm_path, lc_path = _resolve_rasters(
        output_umep, output_data, bbox_wgs84
    )

    # QES inputs: projected/clipped DEM + buildings shapefile + mask
    dem_for_qes, mask_shp = _prepare_qes_dem(dem_path, output_folder, bbox_wgs84)
    print(f"DEM (QES):  {dem_for_qes}")

    buildings_shp = output_folder / "buildings.shp"
    if not buildings_shp.exists():
        print("\nLoading buildings from IGN for QES...")
        buildings_shp = fetch_buildings(output_folder, bbox_wgs84)
    print(f"buildings:  {buildings_shp}")

    if not mask_shp.exists():
        print("ERROR: mask.shp required for QES building preprocessing.")
        sys.exit(1)

    trees_shp = resolve_trees(output_folder, mask_shp, cdsm_path)
    if trees_shp is not None:
        print(f"trees:      {trees_shp}")
    else:
        print("trees:      (none — QES without vegetation)")

    # 1) Precompute 10 QES wind fields
    wind_10dir_dir = output_folder / "wind_10dir"
    wind_10dir_dir.mkdir(parents=True, exist_ok=True)

    print("\nRunning QES-Winds for 10 directions...")
    for d in WIND_DIRECTIONS_DEG:
        dst = wind_10dir_dir / f"wind_speed_{d:03d}.tif"
        if dst.exists():
            print(f"  Skip direction {d}° (exists: {dst.name})")
            continue
        dir_sub = (wind_10dir_dir / f"dir_{d:03d}").resolve()
        dir_sub.mkdir(parents=True, exist_ok=True)
        print(f"  QES direction {d}° ...")
        tif = run_qes_wind(
            dem=dem_for_qes.resolve(),
            buildings_src=buildings_shp.resolve(),
            buildings_mask=mask_shp.resolve(),
            work_dir=dir_sub,
            direction_deg=float(d),
            speed=WIND_SPEED_REF,
            trees_shp=trees_shp,
        )
        shutil.copy2(tif, dst)
        print(f"  QES direction {d}° -> {dst.name}")

    # 2) Load weather and wind direction from EPW
    epw_path = base / "examples" / "la_rochelle_2025.epw"
    if not epw_path.exists():
        epw_path = base / "la_rochelle_2025.epw"
    if not epw_path.exists():
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

    with rasterio.open(dsm_path) as dsm_ref:
        profile = dsm_ref.profile.copy()
        height, width = dsm_ref.height, dsm_ref.width

    profile.update(dtype=rasterio.float32, count=1, nodata=np.nan)
    utci_accum = np.zeros((height, width), dtype=np.float64)
    utci_count = np.zeros((height, width), dtype=np.float64)

    for weather, wdir_deg in zip(weather_list, wind_directions, strict=True):
        ts_str = weather.datetime.strftime("%Y%m%d_%H%M")
        tmrt_path = tmrt_dir / f"tmrt_{ts_str}.tif"
        if not tmrt_path.exists():
            print(f"  Skip {ts_str}: no Tmrt file")
            continue

        idx = _nearest_direction_index(wdir_deg)
        dir_used = WIND_DIRECTIONS_DEG[idx]
        wind_path = wind_10dir_dir / f"wind_speed_{dir_used:03d}.tif"
        print(
            f"  {ts_str} EPW wdir={wdir_deg:.0f}° -> raster {dir_used}°"
        )

        with rasterio.open(tmrt_path) as src:
            tmrt = src.read(1)
        with rasterio.open(wind_path) as src:
            wind_speed_raster = src.read(1)

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

        ws_epw = max(weather.ws, 0.01)
        v_pixel = wind_speed_raster * (ws_epw / WIND_SPEED_REF)

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
        print(f"  {ts_str} -> {out_path.name}")

    mean_utci = np.full((height, width), np.nan, dtype=np.float32)
    valid = utci_count > 0
    mean_utci[valid] = (utci_accum[valid] / utci_count[valid]).astype(np.float32)
    mean_path = output_folder / "utci_mean.tif"
    with rasterio.open(mean_path, "w", **profile) as dst:
        dst.write(mean_utci, 1)
    print(f"\nMean UTCI written: {mean_path}")

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
        ax.set_title("UTCI (QES wind) — mean")
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
