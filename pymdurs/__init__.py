# Import the Rust extension module
import sys

from . import pymdurs as _native
from .pymdurs import *

# `from .pymdurs import *` does not re-export extension submodules, so
# `import pymdurs.geometric` / `from pymdurs.geometric import …` would fail.
for _name in ("geometric", "thermal"):
    _sub = getattr(_native, _name, None)
    if _sub is not None:
        globals()[_name] = _sub
        sys.modules.setdefault(f"{__name__}.{_name}", _sub)

# Pure-Python helpers (tree extraction from CHM); keep separate from Rust geometric.
from . import trees as trees  # noqa: E402

# Python geometric helpers injected onto the Rust `geometric` submodule so
# `from pymdurs.geometric import DetectionUrbanTypes` matches pymdu layout.
from .geometric_helpers import (  # noqa: E402
    LAYER_PROPERTIES,
    TABLE_COLOR_COSIA,
    DetectionUrbanTypes,
    calculate_centroid,
    calculate_circumscribed_circle_radius,
    create_circle,
    dxf_to_cosia_and_weighted_layers,
    dxf_to_polygon_shp,
)

_geom = globals().get("geometric")
if _geom is not None:
    _geom.TABLE_COLOR_COSIA = TABLE_COLOR_COSIA
    _geom.DetectionUrbanTypes = DetectionUrbanTypes
    _geom.LAYER_PROPERTIES = LAYER_PROPERTIES
    _geom.create_circle = create_circle
    _geom.calculate_centroid = calculate_centroid
    _geom.calculate_circumscribed_circle_radius = calculate_circumscribed_circle_radius
    _geom.dxf_to_polygon_shp = dxf_to_polygon_shp
    _geom.dxf_to_cosia_and_weighted_layers = dxf_to_cosia_and_weighted_layers
