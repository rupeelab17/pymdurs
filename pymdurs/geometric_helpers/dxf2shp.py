"""Convert DXF landscape layers to shapefiles and Cosia-weighted overlays.

Port of pymdu ``geometric.Dxf2Shp`` using GeoPandas/Shapely (no osgeo/spaCy).
"""

from __future__ import annotations

import math
import os
from pathlib import Path
from typing import Sequence

import geopandas as gpd
import pandas as pd
from shapely.geometry import LinearRing, LineString, MultiLineString, Point, Polygon
from shapely.geometry.base import BaseGeometry

from .cosia_colors import TABLE_COLOR_COSIA

DEFAULT_DXF_CRS = "EPSG:3946"
TREE_LAYER_SUBSTRING = "ENON-arbres"

# ENON DXF layer → Cosia class + overlay weight (higher wins).
LAYER_PROPERTIES: dict[str, dict[str, float | str]] = {
    "ENON-COUPES": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-arbres 12-14": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres 25-30": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres TRB 12-14": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres existants": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres multitroncs": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres multitroncs 200-250": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres tige 12-14": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres tige 25-30": {"classe": "Feuillu", "weight": 100.0},
    "ENON-arbres tige fruitiers 12-14": {"classe": "Feuillu", "weight": 100.0},
    "ENON-boules de granite": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-clôtures bois opaques": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-copeaux bois Jeux": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-cotations": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-couvre-sol massif": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-couvre-sol pied d'arbre": {"classe": "Pelouse", "weight": 50.0},
    "ENON-engazonnement": {"classe": "Pelouse", "weight": 50.0},
    "ENON-fonte": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-jeux": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-massif arbustif bas": {"classe": "Pelouse", "weight": 50.0},
    "ENON-massifs arbustifs hauts": {"classe": "Feuillu", "weight": 100.0},
    "ENON-massifs boisés micro-forêts": {"classe": "Pelouse", "weight": 50.0},
    "ENON-minéral": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-mobilier bois": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-pavés joints enherbés": {"classe": "Pelouse", "weight": 50.0},
    "ENON-plantations existantes": {"classe": "Pelouse", "weight": 50.0},
    "ENON-plantations existantes à compléter": {"classe": "Pelouse", "weight": 50.0},
    "ENON-ponton estrade terrasse bois": {"classe": "Zone imperméable", "weight": 1.0},
    "ENON-potager (terre végétale)": {"classe": "Pelouse", "weight": 50.0},
    "ENON-sol vert": {"classe": "Pelouse", "weight": 50.0},
    "ENON-vert": {"classe": "Pelouse", "weight": 50.0},
    "ENON-école sols souple jeux": {"classe": "Zone imperméable", "weight": 1.0},
    "MUR": {"classe": "Zone imperméable", "weight": 1.0},
}


def create_circle(
    center: tuple[float, float],
    radius: float,
    num_segments: int = 36,
) -> Polygon:
    """Approximate a circle as a Shapely polygon."""
    angle_step = 2 * math.pi / num_segments
    coords = [
        (
            center[0] + radius * math.cos(i * angle_step),
            center[1] + radius * math.sin(i * angle_step),
        )
        for i in range(num_segments)
    ]
    coords.append(coords[0])
    return Polygon(LinearRing(coords))


def _iter_xy(geometry: BaseGeometry) -> list[tuple[float, float]]:
    """Collect (x, y) vertices from a geometry (Z ignored)."""
    if geometry is None or geometry.is_empty:
        return []
    if isinstance(geometry, Point):
        return [(geometry.x, geometry.y)]
    if isinstance(geometry, (LineString, LinearRing)):
        return [(x, y) for x, y, *_ in geometry.coords]
    if isinstance(geometry, Polygon):
        return _iter_xy(geometry.exterior)
    if hasattr(geometry, "geoms"):
        points: list[tuple[float, float]] = []
        for part in geometry.geoms:
            points.extend(_iter_xy(part))
        return points
    return []


def calculate_centroid(geometry: BaseGeometry) -> tuple[float, float]:
    """Mean of vertices (matches pymdu MultiLineString25D centroid)."""
    points = _iter_xy(geometry)
    if not points:
        raise ValueError("Geometry is empty; cannot compute centroid.")
    n = len(points)
    return (sum(p[0] for p in points) / n, sum(p[1] for p in points) / n)


def calculate_circumscribed_circle_radius(
    geometry: BaseGeometry,
) -> tuple[float, tuple[float, float]]:
    """Radius and center of the axis-aligned bounding-box circumcircle."""
    points = _iter_xy(geometry)
    if not points:
        raise ValueError("Geometry is empty or invalid.")

    min_x = min(p[0] for p in points)
    max_x = max(p[0] for p in points)
    min_y = min(p[1] for p in points)
    max_y = max(p[1] for p in points)
    center = ((min_x + max_x) / 2, (min_y + max_y) / 2)
    radius = max(math.hypot(x - center[0], y - center[1]) for x, y in points)
    return radius, center


def _is_tree_layer(layer_name: object) -> bool:
    return isinstance(layer_name, str) and TREE_LAYER_SUBSTRING in layer_name


def _geometry_to_polygon(geom: BaseGeometry, layer_name: object) -> BaseGeometry | None:
    """Convert a feature geometry; tree layers become circumscribed circles."""
    if geom is None or geom.is_empty:
        return None
    if _is_tree_layer(layer_name):
        radius, _ = calculate_circumscribed_circle_radius(geom)
        center = calculate_centroid(geom)
        return create_circle(center, radius)
    if isinstance(geom, Polygon):
        return geom
    if isinstance(geom, MultiLineString | LineString):
        # Keep as-is; overlay step may buffer later if needed.
        return geom
    return geom


def dxf_to_polygon_shp(
    input_dxf: str | Path,
    output_shp: str | Path,
    encoding: str = "UTF-8",
    crs: str = DEFAULT_DXF_CRS,
) -> gpd.GeoDataFrame:
    """Convert DXF features to a polygon shapefile (trees → circles)."""
    input_dxf = Path(input_dxf)
    output_shp = Path(output_shp)
    if not input_dxf.exists():
        raise FileNotFoundError(f"Cannot open DXF file: {input_dxf}")

    dxf_gdf = gpd.read_file(input_dxf, encoding=encoding)
    if dxf_gdf.empty:
        raise RuntimeError(f"No features found in DXF: {input_dxf}")

    layer_col = "Layer" if "Layer" in dxf_gdf.columns else None
    polygons: list[BaseGeometry] = []
    records: list[dict] = []
    for _, row in dxf_gdf.iterrows():
        layer_name = row[layer_col] if layer_col else None
        new_geom = _geometry_to_polygon(row.geometry, layer_name)
        if new_geom is None:
            continue
        polygons.append(new_geom)
        records.append({c: row[c] for c in dxf_gdf.columns if c != "geometry"})

    out = gpd.GeoDataFrame(records, geometry=polygons, crs=crs)
    output_shp.parent.mkdir(parents=True, exist_ok=True)
    if output_shp.exists():
        # ESRI Shapefile is a multi-file set; GeoPandas overwrites .shp/.dbf/.shx.
        output_shp.unlink(missing_ok=True)
    out.to_file(output_shp, driver="ESRI Shapefile", encoding=encoding)
    return out


def dxf_to_cosia_and_weighted_layers(
    input_shp_dxf: str | Path,
    output_shp_gdf: str | Path,
    bbox_coords: Sequence[float] | None = None,
    bbox_crs: str = "EPSG:4326",
    encoding: str = "UTF-8",
    layer_properties: dict[str, dict[str, float | str]] | None = None,
) -> gpd.GeoDataFrame:
    """Map DXF layers to Cosia classes and dissolve by descending weight."""
    props = layer_properties if layer_properties is not None else LAYER_PROPERTIES
    input_shp_dxf = Path(input_shp_dxf)
    output_shp_gdf = Path(output_shp_gdf)

    dxf_gdf = gpd.read_file(input_shp_dxf, encoding=encoding)
    if dxf_gdf.empty:
        raise RuntimeError(f"SHP from DXF is empty: {input_shp_dxf}")

    if dxf_gdf.crs is None:
        dxf_gdf = dxf_gdf.set_crs(DEFAULT_DXF_CRS)
    else:
        dxf_gdf = dxf_gdf.to_crs(DEFAULT_DXF_CRS)

    if bbox_coords is not None:
        dxf_gdf = dxf_gdf.to_crs(bbox_crs)
        dxf_gdf = dxf_gdf.clip(list(bbox_coords))
        dxf_gdf = dxf_gdf.to_crs(DEFAULT_DXF_CRS)

    if "Layer" not in dxf_gdf.columns:
        raise KeyError("Input shapefile must contain a 'Layer' column.")

    known = dxf_gdf["Layer"].isin(props.keys())
    dxf_gdf = dxf_gdf.loc[known].copy()
    if dxf_gdf.empty:
        raise RuntimeError("No features match known ENON layer_properties keys.")

    dxf_gdf["classe"] = dxf_gdf["Layer"].map(lambda x: props[x]["classe"])
    dxf_gdf["color"] = dxf_gdf["classe"].map(TABLE_COLOR_COSIA)
    dxf_gdf["weight"] = dxf_gdf["Layer"].map(lambda x: props[x]["weight"])

    layer_groups = {layer: data for layer, data in dxf_gdf.groupby("Layer")}
    final_gdf = gpd.GeoDataFrame(columns=dxf_gdf.columns, crs=dxf_gdf.crs)

    for layer, _ in sorted(props.items(), key=lambda x: float(x[1]["weight"]), reverse=True):
        layer_data = layer_groups.get(layer)
        if layer_data is None:
            continue
        layer_data = layer_data.copy()
        layer_data["geometry"] = layer_data["geometry"].buffer(0)
        if not final_gdf.empty:
            layer_data = gpd.overlay(layer_data, final_gdf, how="difference")
        final_gdf = pd.concat([final_gdf, layer_data], ignore_index=True)

    final_gdf = gpd.GeoDataFrame(final_gdf, geometry="geometry", crs=dxf_gdf.crs)
    output_dir = output_shp_gdf.parent
    if output_dir and not output_dir.exists():
        os.makedirs(output_dir, exist_ok=True)
    final_gdf.to_file(output_shp_gdf, driver="ESRI Shapefile", encoding=encoding)
    return final_gdf
