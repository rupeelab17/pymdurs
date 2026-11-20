"""Basic tests for pymdurs Python bindings"""

import pytest


def test_bounding_box():
    """Test BoundingBox creation and properties"""
    import pymdurs

    Bbox = pymdurs.BoundingBox(-1.0, 46.0, 1.0, 47.0)
    assert Bbox.min_x == -1.0
    assert Bbox.min_y == 46.0
    assert Bbox.max_x == 1.0
    assert Bbox.max_y == 47.0


def test_geo_core():
    """Test GeoCore creation and properties"""
    import pymdurs

    geo = pymdurs.GeoCore(epsg=2154)
    assert geo.epsg == 2154

    geo.set_epsg(4326)
    assert geo.epsg == 4326

    geo.set_output_path("./output")
    assert geo.output_path == "./output"


def test_building_set_bbox_updates_geo_core():
    """set_bbox on Building must sync geo_core.bbox"""
    import pymdurs

    buildings = pymdurs.geometric.Building(output_path="./output")
    buildings.set_bbox(-1.15, 46.18, -1.14, 46.19)
    bbox = buildings.geo_core.bbox
    assert bbox is not None
    assert bbox.min_x == -1.15
    assert bbox.min_y == 46.18
    assert bbox.max_x == -1.14
    assert bbox.max_y == 46.19


def test_building_creation():
    """Test BuildingCollection creation"""
    import pymdurs

    building = pymdurs.geometric.Building(
        output_path="./output", defaultStoreyHeight=3.0
    )
    assert building is not None
    assert len(building) == 0


def test_building_to_pandas(tmp_path):
    """Native PyO3 conversion preserves numbers, missing values and booleans."""
    import json

    import pandas as pd
    import pymdurs

    geometry = {
        "type": "Polygon",
        "coordinates": [[[0, 0], [2, 0], [2, 2], [0, 2], [0, 0]]],
    }
    payload = {
        "type": "FeatureCollection",
        "features": [
            {"type": "Feature", "geometry": geometry, "properties": properties}
            for properties in (
                {"hauteur": 12, "nombre_d_etages": 4, "hauteur_2": 10},
                {},
            )
        ],
    }
    buildings = pymdurs.geometric.Building.from_geojson(
        json.dumps(payload).encode(), str(tmp_path), 3.0, None
    )
    df = buildings.to_pandas()
    assert list(df.columns) == [
        "hauteur",
        "area",
        "centroid_x",
        "centroid_y",
        "nombre_d_etages",
        "hauteur_2",
        "noHauteur",
    ]
    assert df["area"].tolist() == [4.0, 4.0]
    assert df["centroid_x"].tolist() == df["centroid_y"].tolist() == [1.0, 1.0]
    for column, value in (
        ("hauteur", 12.0),
        ("nombre_d_etages", 4.0),
        ("hauteur_2", 10.0),
    ):
        assert df.loc[0, column] == value
        assert pd.isna(df.loc[1, column])
    assert df["noHauteur"].tolist() == [False, True]
    assert df["noHauteur"].dtype == bool
    assert pymdurs.geometric.Building(output_path=str(tmp_path)).to_pandas().empty


def test_dem_creation():
    """Test Dem creation"""
    import pymdurs

    dem = pymdurs.geometric.Dem(output_path="./output")
    assert dem is not None


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
