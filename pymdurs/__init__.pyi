from pathlib import Path
from typing import Sequence

from geopandas import GeoDataFrame
from shapely.geometry import Polygon
from shapely.geometry.base import BaseGeometry
from typing_extensions import Self

from . import indicators
from . import vegetation
from .pymdurs import (
    BoundingBox,
    GeoCore,
    PyBoundingBox,
    PyGeoCore,
    geometric,
    thermal,
)

TABLE_COLOR_COSIA: dict[str, str]
LAYER_PROPERTIES: dict[str, dict[str, float | str]]

class DetectionUrbanTypes:
    output_path: str
    gdf: GeoDataFrame | None

    def __init__(self, output_path: str | None = ...) -> None: ...
    def set_bbox(self, min_x: float, min_y: float, max_x: float, max_y: float) -> None: ...
    def set_crs(self, epsg: int) -> None: ...
    @property
    def bbox(self) -> list[float] | None: ...
    @bbox.setter
    def bbox(self, value: list[float]) -> None: ...
    @property
    def epsg(self) -> int: ...
    @epsg.setter
    def epsg(self, value: int) -> None: ...
    def run(self, nbr_cluster: int = ...) -> Self: ...
    def to_gdf(self) -> GeoDataFrame: ...
    def to_gpkg(self, name: str = ...) -> str: ...

def calculate_centroid(geometry: BaseGeometry) -> tuple[float, float]: ...
def calculate_circumscribed_circle_radius(geometry: BaseGeometry) -> tuple[float, tuple[float, float]]: ...
def create_circle(center: tuple[float, float], radius: float, num_segments: int = ...) -> Polygon: ...
def dxf_to_polygon_shp(
    input_dxf: str | Path,
    output_shp: str | Path,
    encoding: str = ...,
    crs: str = ...,
) -> GeoDataFrame: ...
def dxf_to_cosia_and_weighted_layers(
    input_shp_dxf: str | Path,
    output_shp_gdf: str | Path,
    bbox_coords: Sequence[float] | None = ...,
    bbox_crs: str = ...,
    encoding: str = ...,
    layer_properties: dict[str, dict[str, float | str]] | None = ...,
) -> GeoDataFrame: ...

__all__ = [
    "BoundingBox",
    "GeoCore",
    "PyBoundingBox",
    "PyGeoCore",
    "geometric",
    "thermal",
    "indicators",
    "vegetation",
    "TABLE_COLOR_COSIA",
    "LAYER_PROPERTIES",
    "DetectionUrbanTypes",
    "calculate_centroid",
    "calculate_circumscribed_circle_radius",
    "create_circle",
    "dxf_to_polygon_shp",
    "dxf_to_cosia_and_weighted_layers",
]
