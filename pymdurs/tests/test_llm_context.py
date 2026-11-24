"""Test the public, offline context and its packaged workflow."""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest
import rasterio
from rasterio.transform import from_origin

import pymdurs
from pymdurs.examples import utci_bbox

_ROOT = Path(__file__).resolve().parents[2]


def test_llm_context_includes_typed_stubs_and_workflow() -> None:
    context = pymdurs.llm_context()
    assert context == (_ROOT / "llms.txt").read_text(encoding="utf-8")
    assert "min_x: builtins.float" in context
    assert "`RUST_LOG` wins when set." in context
    assert "pymdurs/geometric/__init__.pyi" in context
    assert "pymdurs/thermal/__init__.pyi" in context
    assert "solweig.SurfaceData.prepare(" in context
    assert "connect_chatgpt" not in context
    assert not hasattr(pymdurs, "ChatGPTSession")


def test_bordeaux_utci_workflow_produces_georeferenced_rasters(tmp_path: Path) -> None:
    pytest.importorskip("solweig")
    bbox = utci_bbox._bbox()
    dem = np.full((200, 200), 20.0, dtype=np.float32)
    dsm = dem.copy()
    dsm[90:105, 90:105] += 12.0  # Synthetic building in absolute elevation metres.
    for name, values in (("dem", dem), ("dsm", dsm)):
        with rasterio.open(
            tmp_path / f"{name}.tif",
            "w",
            driver="GTiff",
            height=200,
            width=200,
            count=1,
            dtype="float32",
            crs="EPSG:2154",
            transform=from_origin(bbox.min_x, bbox.max_y, 5, 5),
        ) as dst:
            dst.write(values, 1)

    output = tmp_path / "result"
    utci_bbox.main(
        [
            "--dem",
            str(tmp_path / "dem.tif"),
            "--dsm",
            str(tmp_path / "dsm.tif"),
            "--epw",
            str(_ROOT / "examples/bordeaux_2025.epw"),
            "--when",
            "2025-07-01 13:00",
            "--output",
            str(output),
        ]
    )
    for name in ("tmrt", "utci"):
        (path,) = (output / name).glob(f"{name}_*.tif")
        with rasterio.open(path) as src:
            assert src.shape == (200, 200)
            assert src.crs == rasterio.crs.CRS.from_epsg(2154)
            assert src.bounds == pytest.approx(
                (bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y)
            )
            assert np.isfinite(src.read(1, masked=True).compressed()).any()
