"""Regression checks for the UMEP raster preparation example."""

from __future__ import annotations

import logging
import os
from pathlib import Path

import geopandas as gpd
import numpy as np
import pytest
import rasterio
from rasterio.enums import Resampling
from rasterio.transform import from_origin
from shapely.geometry import Polygon, box

from examples.umep_rasters import fill_dsm_nodata, surface_cache_directory
from examples.utils import warp_clip_raster


def write_raster(path: Path, values: np.ndarray, *, crs: str = "EPSG:2975") -> None:
    """Write a small striped raster, like the workflow's clipped inputs."""
    with rasterio.open(
        path,
        "w",
        driver="GTiff",
        width=values.shape[2],
        height=values.shape[1],
        count=values.shape[0],
        dtype="float32",
        crs=crs,
        transform=from_origin(345583, 7647784, 1, 1),
        nodata=-9999.0,
    ) as dst:
        dst.write(values.astype(np.float32))


def test_surface_cache_tracks_contents_not_mtime(tmp_path: Path) -> None:
    raster = tmp_path / "DSM.tif"
    values = np.ones((1, 2, 2), dtype=np.float32)
    write_raster(raster, values, crs="EPSG:2154")
    working = tmp_path / "working"
    old_cache = surface_cache_directory(working, raster)
    assert old_cache.parent == working
    assert old_cache != working
    os.utime(raster, ns=(1, 1))
    assert surface_cache_directory(working, raster) == old_cache

    write_raster(raster, values)
    new_cache = surface_cache_directory(working, raster)
    assert new_cache != old_cache
    values[0, 0, 0] = 2
    write_raster(raster, values)
    assert surface_cache_directory(working, raster) != new_cache

    cdsm = tmp_path / "CDSM.tif"
    write_raster(cdsm, values)
    cache = surface_cache_directory(working, raster, cdsm)
    values[0, 0, 1] = 3
    write_raster(cdsm, values)
    assert surface_cache_directory(working, raster, cdsm) != cache


def test_fill_dsm_nodata_preserves_valid_elevations(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    dsm = tmp_path / "DSM.tif"
    dem = tmp_path / "DEM.tif"
    values = np.array([[[0, -9999, 12], [np.nan, 0, 7]]], dtype=np.float32)
    write_raster(dsm, np.repeat(values, 3, axis=0))
    write_raster(dem, np.array([[[5, 6, 2], [8, -9999, 0]]]))
    with caplog.at_level(logging.WARNING):
        assert fill_dsm_nodata(dsm, dem) == (3, 6)
    assert "BLOCKXSIZE" not in caplog.text
    with rasterio.open(dsm) as src:
        assert src.count == 1
        assert src.nodata == -9999.0
        np.testing.assert_array_equal(src.read(1), [[5, 6, 12], [8, -9999, 7]])
    assert fill_dsm_nodata(dsm, dem) == (0, 6)


def test_warp_clip_keeps_exact_resolution_and_nodata(tmp_path: Path) -> None:
    source = tmp_path / "source.tif"
    target = tmp_path / "clipped.tif"
    mask = tmp_path / "mask.geojson"
    values = np.array([[[0, -9999], [7, 9]]], dtype=np.float32)
    write_raster(source, np.repeat(values, 2, axis=0))
    gpd.GeoDataFrame(
        geometry=[box(345583, 7647781.4, 345585.6, 7647784)], crs="EPSG:2975"
    ).to_file(mask, driver="GeoJSON")

    warp_clip_raster(source, target, mask, dst_crs="EPSG:2975")
    with rasterio.open(target) as dst:
        assert dst.res == (1.0, 1.0)
        assert dst.shape == (3, 3)
        assert dst.count == 2
        assert dst.nodata == -9999.0
        expected = [[0, -9999, -9999], [7, 9, -9999], [-9999, -9999, -9999]]
        np.testing.assert_array_equal(dst.read(1), expected)
        np.testing.assert_array_equal(dst.read(2), expected)
        assert not dst.read(1, masked=True).mask[0, 0]  # Zero is a valid elevation.


def test_warp_clip_masks_polygon_not_just_bounds(tmp_path: Path) -> None:
    source = tmp_path / "source.tif"
    target = tmp_path / "clipped.tif"
    mask = tmp_path / "mask.geojson"
    write_raster(source, np.full((1, 2, 2), 7))
    polygon = Polygon([(345583, 7647784), (345585, 7647784), (345583, 7647782)])
    gpd.GeoDataFrame(geometry=[polygon], crs="EPSG:2975").to_file(
        mask, driver="GeoJSON"
    )
    warp_clip_raster(source, target, mask, dst_crs="EPSG:2975")
    with rasterio.open(target) as dst:
        assert dst.read(1)[0, 0] == 7
        assert dst.read(1)[1, 1] == dst.nodata


def test_warp_clip_reprojects_mask_and_preserves_classes(tmp_path: Path) -> None:
    source = tmp_path / "classes.tif"
    target = tmp_path / "clipped.tif"
    mask = tmp_path / "mask.geojson"
    write_raster(source, np.array([[[1, 4], [4, 1]]]))
    gpd.GeoDataFrame(
        geometry=[box(345583, 7647782, 345585, 7647784)], crs="EPSG:2975"
    ).to_crs(4326).to_file(mask, driver="GeoJSON")
    warp_clip_raster(
        source,
        target,
        mask,
        dst_crs="EPSG:2975",
        resolution=0.6,
        resampling=Resampling.nearest,
    )
    with rasterio.open(target) as dst:
        assert dst.res == (0.6, 0.6)
        assert set(dst.read(1).ravel()) <= {1, 4, -9999}


@pytest.mark.parametrize("resolution", [0, -1, np.nan, np.inf])
def test_warp_clip_rejects_invalid_resolution(
    tmp_path: Path, resolution: float
) -> None:
    with pytest.raises(ValueError, match="resolution must be finite and positive"):
        warp_clip_raster(
            "unused.tif", tmp_path / "output.tif", "unused.shp", resolution=resolution
        )


def test_fill_dsm_nodata_rejects_different_grids(tmp_path: Path) -> None:
    dsm = tmp_path / "DSM.tif"
    dem = tmp_path / "DEM.tif"
    values = np.ones((1, 2, 2), dtype=np.float32)
    write_raster(dsm, values)
    write_raster(dem, values, crs="EPSG:2154")
    with pytest.raises(ValueError, match="same CRS, transform and shape"):
        fill_dsm_nodata(dsm, dem)
