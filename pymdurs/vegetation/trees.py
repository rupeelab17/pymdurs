"""Extract individual trees from a LiDAR CHM and write a point shapefile.

Inspired by pymdu ``Lidar.extract_tree_crowns`` / ``run_trees`` (watershed on CHM),
adapted for QES-style attributes: point geometry with fields ``H``, ``D``, ``LAI``.
"""

from __future__ import annotations

import math
from pathlib import Path

import geopandas as gpd
import numpy as np
import rasterio
import rasterio.features
from rasterio.transform import Affine, xy
from shapely.geometry import Point, shape
from skimage.feature import peak_local_max
from skimage.segmentation import watershed

DEFAULT_LAI = 4.0
DEFAULT_CLASSIFICATION_LIST = [2, 3, 4, 5]
DEFAULT_CRS = "EPSG:2154"


def extract_tree_crowns(
    chm: np.ndarray,
    transform: Affine,
    *,
    min_tree_height: float = 2.0,
    min_distance: int = 5,
    lai: float = DEFAULT_LAI,
    crs: str = DEFAULT_CRS,
) -> gpd.GeoDataFrame:
    """Extract tree top points from a CHM using local maxima + watershed.

    Args:
        chm: 2D canopy height model (north-up), heights in metres.
        transform: Affine transform of ``chm``.
        min_tree_height: Minimum CHM height (m) to treat as canopy.
        min_distance: Minimum pixel distance between tree tops.
        lai: Constant leaf-area index written to every feature.
        crs: Output CRS (default Lambert-93).

    Returns:
        GeoDataFrame of points with columns ``H``, ``D``, ``LAI``.
        ``H`` is max CHM in the watershed segment; ``D`` is equivalent
        crown diameter ``2 * sqrt(area / pi)``.
    """
    chm = np.asarray(chm, dtype=np.float64)
    if chm.ndim != 2:
        raise ValueError(f"chm must be 2D, got shape {chm.shape}")

    # Treat NaN / non-finite as ground for peak detection
    chm_work = np.nan_to_num(chm, nan=0.0, posinf=0.0, neginf=0.0)
    mask = chm_work >= min_tree_height

    local_max_coords = peak_local_max(
        chm_work,
        min_distance=min_distance,
        threshold_abs=min_tree_height,
    )

    empty = gpd.GeoDataFrame(
        {"H": [], "D": [], "LAI": []},
        geometry=[],
        crs=crs,
    )
    if len(local_max_coords) == 0:
        return empty

    markers = np.zeros_like(chm_work, dtype=np.int32)
    for idx, (row, col) in enumerate(local_max_coords, start=1):
        markers[row, col] = idx

    segmentation = watershed(-chm_work, markers, mask=mask)

    # Segment id -> crown area (m²) from polygonized footprints
    area_by_id: dict[int, float] = {}
    for geom, val in rasterio.features.shapes(
        segmentation.astype(np.int32),
        mask=(segmentation > 0),
        transform=transform,
    ):
        seg_id = int(val)
        if seg_id == 0:
            continue
        poly = shape(geom)
        area_by_id[seg_id] = area_by_id.get(seg_id, 0.0) + float(poly.area)

    points: list[Point] = []
    heights: list[float] = []
    diameters: list[float] = []
    lais: list[float] = []

    for idx, (row, col) in enumerate(local_max_coords, start=1):
        seg_mask = segmentation == idx
        if not np.any(seg_mask):
            continue

        height = float(chm_work[seg_mask].max())
        area = area_by_id.get(idx, 0.0)
        if height < min_tree_height or area <= 0.0:
            continue

        diameter = 2.0 * math.sqrt(area / math.pi)
        x, y = xy(transform, row, col)
        points.append(Point(x, y))
        heights.append(height)
        diameters.append(diameter)
        lais.append(float(lai))

    if not points:
        return empty

    return gpd.GeoDataFrame(
        {"H": heights, "D": diameters, "LAI": lais},
        geometry=points,
        crs=crs,
    )


def run_trees(
    lidar,
    *,
    file_name: str = "trees.shp",
    classification_list: list[int] | None = None,
    resolution: float = 1.0,
    min_tree_height: float = 2.0,
    min_distance: int = 5,
    lai: float = DEFAULT_LAI,
    write_chm: bool = True,
    chm_file_name: str = "lidar_cdsm.tif",
) -> Path:
    """Build CHM via ``lidar.run``, extract trees, write point shapefile.

    Args:
        lidar: ``pymdurs.geometric.Lidar`` instance (bbox already set).
        file_name: Output shapefile name (under ``lidar.get_output_path()``).
        classification_list: ASPRS classes for CHM (default ground + vegetation).
        resolution: Pixel size in metres.
        min_tree_height: Minimum canopy height (m).
        min_distance: Minimum pixel distance between tops.
        lai: Constant LAI for all trees.
        write_chm: Whether to write the intermediate multi-band GeoTIFF.
        chm_file_name: Intermediate GeoTIFF name (bands: DSM, DTM, CHM).

    Returns:
        Absolute path to the written shapefile.
    """
    classes = classification_list if classification_list is not None else list(
        DEFAULT_CLASSIFICATION_LIST
    )

    tif_path = lidar.run(
        file_name=chm_file_name,
        classification_list=classes,
        resolution=resolution,
        write_out_file=write_chm,
    )
    tif_path = Path(tif_path)
    if not tif_path.is_file():
        raise FileNotFoundError(
            f"CHM GeoTIFF not found at {tif_path}. "
            "Ensure write_chm=True or that lidar.run wrote the file."
        )

    with rasterio.open(tif_path) as src:
        # Band 3 = CHM (see Lidar.to_tif)
        chm = src.read(3).astype(np.float64)
        transform = src.transform
        crs = src.crs.to_string() if src.crs else DEFAULT_CRS

    gdf = extract_tree_crowns(
        chm,
        transform,
        min_tree_height=min_tree_height,
        min_distance=min_distance,
        lai=lai,
        crs=crs if crs else DEFAULT_CRS,
    )

    out_dir = Path(lidar.get_output_path())
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = Path(file_name)
    if not out_path.is_absolute():
        out_path = out_dir / out_path

    # Remove stale shapefile sidecars before rewrite
    for ext in (".shp", ".shx", ".dbf", ".prj", ".cpg"):
        sibling = out_path.with_suffix(ext)
        if sibling.exists():
            sibling.unlink()

    gdf.to_file(out_path, driver="ESRI Shapefile")
    return out_path.resolve()
