"""Unit tests for CHM tree extraction (no network / LiDAR download)."""

from __future__ import annotations

import math

import numpy as np
from rasterio.transform import from_origin

from pymdurs.vegetation import extract_tree_crowns


def test_extract_tree_crowns_synthetic_peaks():
    # 30x30 CHM with two Gaussian-like crowns
    chm = np.zeros((30, 30), dtype=np.float64)
    for r, c, h in ((10, 10, 12.0), (10, 22, 8.0)):
        for i in range(30):
            for j in range(30):
                d2 = (i - r) ** 2 + (j - c) ** 2
                val = h * math.exp(-d2 / 8.0)
                if val > chm[i, j]:
                    chm[i, j] = val

    transform = from_origin(400000.0, 6500030.0, 1.0, 1.0)
    gdf = extract_tree_crowns(
        chm,
        transform,
        min_tree_height=2.0,
        min_distance=5,
        lai=4.0,
    )

    assert gdf.crs is not None
    assert str(gdf.crs).endswith("2154") or gdf.crs.to_epsg() == 2154
    assert list(gdf.columns) == ["H", "D", "LAI", "geometry"]
    assert len(gdf) >= 2
    assert (gdf["H"] >= 2.0).all()
    assert (gdf["D"] > 0).all()
    assert (gdf["LAI"] == 4.0).all()
    assert gdf.geometry.geom_type.eq("Point").all()


def test_extract_tree_crowns_empty():
    chm = np.zeros((10, 10), dtype=np.float64)
    transform = from_origin(0.0, 10.0, 1.0, 1.0)
    gdf = extract_tree_crowns(chm, transform, min_tree_height=2.0)
    assert len(gdf) == 0
    assert list(gdf.columns) == ["H", "D", "LAI", "geometry"]
