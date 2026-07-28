"""Pure-Python geometric helpers (injected onto ``pymdurs.geometric`` at import)."""

from .cosia_colors import TABLE_COLOR_COSIA
from .detection_urban_types import DetectionUrbanTypes
from .dxf2shp import (
    LAYER_PROPERTIES,
    calculate_centroid,
    calculate_circumscribed_circle_radius,
    create_circle,
    dxf_to_cosia_and_weighted_layers,
    dxf_to_polygon_shp,
)

__all__ = [
    "TABLE_COLOR_COSIA",
    "DetectionUrbanTypes",
    "LAYER_PROPERTIES",
    "calculate_centroid",
    "calculate_circumscribed_circle_radius",
    "create_circle",
    "dxf_to_cosia_and_weighted_layers",
    "dxf_to_polygon_shp",
]
