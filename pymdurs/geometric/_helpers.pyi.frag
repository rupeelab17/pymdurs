# --- Pure-Python geometric helpers (injected at runtime via pymdurs.__init__) ---

TABLE_COLOR_COSIA: dict[builtins.str, builtins.str]
LAYER_PROPERTIES: dict[builtins.str, dict[builtins.str, builtins.float | builtins.str]]

def create_circle(
    center: tuple[builtins.float, builtins.float],
    radius: builtins.float,
    num_segments: builtins.int = 36,
) -> typing.Any: ...

def calculate_centroid(geometry: typing.Any) -> tuple[builtins.float, builtins.float]: ...

def calculate_circumscribed_circle_radius(
    geometry: typing.Any,
) -> tuple[builtins.float, tuple[builtins.float, builtins.float]]: ...

def dxf_to_polygon_shp(
    input_dxf: builtins.str | typing.Any,
    output_shp: builtins.str | typing.Any,
    encoding: builtins.str = "UTF-8",
    crs: builtins.str = "EPSG:3946",
) -> geopandas.GeoDataFrame: ...

def dxf_to_cosia_and_weighted_layers(
    input_shp_dxf: builtins.str | typing.Any,
    output_shp_gdf: builtins.str | typing.Any,
    bbox_coords: typing.Optional[typing.Sequence[builtins.float]] = None,
    bbox_crs: builtins.str = "EPSG:4326",
    encoding: builtins.str = "UTF-8",
    layer_properties: typing.Optional[dict[builtins.str, dict[builtins.str, builtins.float | builtins.str]]] = None,
) -> geopandas.GeoDataFrame: ...

@typing.final
class DetectionUrbanTypes:
    r"""
    Cluster buildings into urban morphotypes (momepy + Clustergram).
    """
    output_path: builtins.str
    gdf: typing.Optional[geopandas.GeoDataFrame]
    def __init__(self, output_path: typing.Optional[builtins.str] = None) -> None: ...
    def set_bbox(
        self,
        min_x: builtins.float,
        min_y: builtins.float,
        max_x: builtins.float,
        max_y: builtins.float,
    ) -> None: ...
    def set_crs(self, epsg: builtins.int) -> None: ...
    @property
    def bbox(self) -> typing.Optional[list[builtins.float]]: ...
    @bbox.setter
    def bbox(self, value: list[builtins.float]) -> None: ...
    @property
    def epsg(self) -> builtins.int: ...
    @epsg.setter
    def epsg(self, value: builtins.int) -> None: ...
    def run(self, nbr_cluster: builtins.int = 4) -> Self: ...
    def to_gdf(self) -> geopandas.GeoDataFrame: ...
    def to_gpkg(self, name: builtins.str = "detection") -> builtins.str: ...
