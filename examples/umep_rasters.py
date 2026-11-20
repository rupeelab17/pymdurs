"""Raster preparation and cache isolation for the UMEP example."""

from __future__ import annotations

import hashlib
from pathlib import Path

import geopandas as gpd
import numpy as np
import rasterio
from rasterio.enums import Resampling
from rasterio.mask import mask as rio_mask
from rasterio.transform import from_origin
from rasterio.vrt import WarpedVRT


def warp_clip_raster(
    src_path: str | Path,
    dst_path: str | Path,
    mask_shp: str | Path,
    *,
    dst_crs: str = "EPSG:2154",
    resolution: float = 1.0,
    resampling: Resampling = Resampling.bilinear,
) -> None:
    """Warp onto an exact-resolution grid and mask without inventing zero elevations.

    Args:
        src_path: Georeferenced input raster.
        dst_path: Output Float32 GeoTIFF, with NoData set to -9999.
        mask_shp: Polygon mask with a defined CRS.
        dst_crs: Target projected coordinate reference system.
        resolution: Positive target pixel size in target CRS units.
        resampling: Interpolation method; use nearest for land-cover classes.

    Raises:
        ValueError: If resolution, mask extent or source CRS is invalid.
    """
    if not np.isfinite(resolution) or resolution <= 0:
        raise ValueError("resolution must be finite and positive")
    geometries = gpd.read_file(mask_shp).to_crs(dst_crs)
    left, bottom, right, top = geometries.total_bounds
    if (
        not np.isfinite([left, bottom, right, top]).all()
        or right <= left
        or top <= bottom
    ):
        raise ValueError("Mask must have a non-empty, finite extent")
    width = int(np.ceil((right - left) / resolution))
    height = int(np.ceil((top - bottom) / resolution))
    transform = from_origin(left, top, resolution, resolution)
    nodata = -9999.0

    with rasterio.open(src_path) as src:
        if src.crs is None:
            raise ValueError("Source raster must have a CRS")
        with WarpedVRT(
            src,
            crs=dst_crs,
            transform=transform,
            width=width,
            height=height,
            dtype="float32",
            nodata=nodata,
            resampling=resampling,
        ) as warped:
            clipped, _ = rio_mask(
                warped, geometries.geometry, crop=False, nodata=nodata, filled=True
            )

    dst_path = Path(dst_path)
    dst_path.parent.mkdir(parents=True, exist_ok=True)
    with rasterio.open(
        dst_path,
        "w",
        driver="GTiff",
        width=width,
        height=height,
        count=clipped.shape[0],
        dtype="float32",
        crs=dst_crs,
        transform=transform,
        nodata=nodata,
    ) as dst:
        dst.write(clipped)


def surface_cache_directory(working_dir: Path, *rasters: Path) -> Path:
    """Key preprocessing caches by input contents, including GeoTIFF georeferencing.

    Args:
        working_dir: Parent directory for preprocessing caches.
        *rasters: Input rasters in a stable order (DSM, CDSM, land cover).

    Returns:
        A cache path that changes whenever any input file changes.
    """
    fingerprint = hashlib.sha256()
    for path in rasters:
        with path.open("rb") as source:
            fingerprint.update(hashlib.file_digest(source, "sha256").digest())
    return working_dir / fingerprint.hexdigest()


def fill_dsm_nodata(dsm_path: Path, dem_path: Path) -> tuple[int, int]:
    """Fill missing DSM elevations from a DEM on the same grid.

    Args:
        dsm_path: DSM to update in place; only its elevation band is retained.
        dem_path: DEM providing valid replacement elevations.

    Returns:
        Number of filled pixels and total number of DSM pixels.

    Raises:
        ValueError: If the DSM and DEM do not share the same grid.
    """
    with rasterio.open(dem_path) as dem, rasterio.open(dsm_path) as dsm:
        if (dem.crs, dem.transform, dem.shape) != (dsm.crs, dsm.transform, dsm.shape):
            raise ValueError("DSM and DEM must have the same CRS, transform and shape")
        dem_data = dem.read(1, masked=True)
        dsm_data = dsm.read(1, masked=True)
        profile = dsm.profile.copy()

    invalid = (
        np.ma.getmaskarray(dsm_data)
        | ~np.isfinite(dsm_data.data)
        | (dsm_data.data == 0)
    )
    dem_valid = ~np.ma.getmaskarray(dem_data) & np.isfinite(dem_data.data)
    fill_mask = invalid & dem_valid
    filled_count = int(np.count_nonzero(fill_mask))
    if filled_count:
        values = dsm_data.data.astype(np.float32)
        values[invalid] = -9999.0
        values[fill_mask] = dem_data.data[fill_mask]
        profile.update(count=1, dtype="float32", nodata=-9999.0)
        if not profile.get("tiled", False):
            profile.pop("blockxsize", None)
            profile.pop("blockysize", None)
        with rasterio.open(dsm_path, "w", **profile) as dst:
            dst.write(values, 1)
    return filled_count, dsm_data.size
