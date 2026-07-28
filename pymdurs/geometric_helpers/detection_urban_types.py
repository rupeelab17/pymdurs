"""Detect urban morphotype clusters from buildings and street networks.

Port of pymdu ``geometric.DetectionUrbanTypes``, adapted to pymdurs APIs
(``set_bbox`` / ``set_crs`` / ``Building.get_geojson``) and momepy >= 1.0
(functional API + ``libpysal.graph.Graph``). Heavy deps are lazy.
"""

from __future__ import annotations

import os
from typing import Self

import geopandas as gpd
import pandas as pd
from shapely import box

DEFAULT_TEMP_PATH = "./temp"
DEFAULT_EPSG = 2154


def _require_urban_deps() -> tuple:
    """Import optional morphometrics stack; raise a clear error if missing."""
    try:
        import matplotlib.pyplot as plt
        import momepy
        import osmnx
        from clustergram import Clustergram
        from libpysal.graph import Graph
    except ImportError as exc:
        raise ImportError(
            "DetectionUrbanTypes requires optional deps. Install with: "
            'pip install "pymdurs[urban]"'
        ) from exc
    return plt, momepy, osmnx, Clustergram, Graph


def _load_buildings(
    bbox_final: list[float],
    output_path: str,
    local_crs: int,
) -> gpd.GeoDataFrame:
    """Fetch IGN buildings for bbox and project to local CRS (index = uID)."""
    from pandas.api.types import is_datetime64_any_dtype as is_datetime

    from pymdurs.geometric import Building

    buildings_api = Building(output_path=output_path)
    buildings_api.set_bbox(*bbox_final)
    buildings_api.run()
    geojson = buildings_api.get_geojson()
    buildings = gpd.GeoDataFrame.from_features(
        geojson["features"],
        crs="EPSG:4326",
    )
    buildings = buildings[
        [c for c in buildings.columns if not is_datetime(buildings[c])]
    ]
    buildings = buildings.to_crs(local_crs)
    buildings = buildings.explode(index_parts=False, ignore_index=True)
    buildings["uID"] = range(len(buildings))
    return buildings.set_index("uID", drop=False)


def _expand_bbox(bbox: list[float], scale: float = 1.15) -> list[float]:
    """Scale WGS84 bbox around its center and return [minx, miny, maxx, maxy]."""
    gdf_project = gpd.GeoDataFrame(
        gpd.GeoSeries(box(bbox[0], bbox[1], bbox[2], bbox[3])),
        columns=["geometry"],
        crs="epsg:4326",
    )
    gdf_project = gdf_project.scale(xfact=scale, yfact=scale)
    bounds = gdf_project.envelope.bounds.values[0]
    return [float(bounds[0]), float(bounds[1]), float(bounds[2]), float(bounds[3])]


def _fetch_streets(envelope_polygon, local_crs: int, osmnx, momepy) -> gpd.GeoDataFrame:
    """Download OSM drive network clipped to polygon, projected to local CRS."""
    osmnx.settings.log_console = True
    osmnx.settings.overpass_url = "https://overpass-api.de/api"

    custom_filters = (
        '["area"!~"yes"]["highway"~"footway|pedestrian|cycleway"]'
        '["foot"!~"no"]["service"!~"private"]{}'
    ).format(osmnx.settings.default_access)

    osm_graph = osmnx.graph_from_polygon(
        envelope_polygon,
        network_type="drive",
        truncate_by_edge=True,
        simplify=True,
        custom_filter=custom_filters,
    )
    osm_graph = osmnx.project_graph(osm_graph, to_crs=local_crs)
    streets = osmnx.graph_to_gdfs(
        osm_graph,
        nodes=False,
        edges=True,
        node_geometry=False,
        fill_edge_geometry=True,
    )
    streets = momepy.remove_false_nodes(streets)
    streets = streets[["geometry"]].copy().reset_index(drop=True)
    streets["nID"] = streets.index
    return streets


def _align_buildings_to_tessellation(
    buildings: gpd.GeoDataFrame,
    tessellation: gpd.GeoDataFrame,
) -> tuple[gpd.GeoDataFrame, gpd.GeoDataFrame]:
    """Keep only buildings that received a tessellation cell (shared index)."""
    common = buildings.index.intersection(tessellation.index)
    if len(common) == 0:
        raise RuntimeError("No buildings overlap morphological tessellation cells.")
    if len(common) < len(buildings):
        dropped = len(buildings) - len(common)
        print(
            f"Dropping {dropped} building(s) without tessellation cells "
            f"({len(common)} kept)."
        )
    buildings = buildings.loc[common].copy()
    tessellation = tessellation.loc[common].copy()
    tessellation["uID"] = tessellation.index
    buildings["uID"] = buildings.index
    return buildings, tessellation


def _compute_morphometrics(
    buildings: gpd.GeoDataFrame,
    streets: gpd.GeoDataFrame,
    momepy,
    Graph,
) -> tuple[gpd.GeoDataFrame, gpd.GeoDataFrame, object]:
    """Build tessellation and morphometric features (momepy >= 1.0)."""
    limit = momepy.buffered_limit(buildings, buffer=100)
    tessellation = momepy.morphological_tessellation(
        buildings, clip=limit, segment=1.0
    )
    buildings, tessellation = _align_buildings_to_tessellation(
        buildings, tessellation
    )
    streets = streets.copy()

    buildings["area"] = buildings.area
    tessellation["area"] = tessellation.area
    streets["length"] = streets.length

    buildings["eri"] = momepy.equivalent_rectangular_index(buildings)
    buildings["elongation"] = momepy.elongation(buildings)
    tessellation["convexity"] = momepy.convexity(tessellation)
    streets["linearity"] = momepy.linearity(streets)
    perimeter = buildings.length.replace(0, pd.NA)
    buildings["shared_walls"] = momepy.shared_walls(buildings) / perimeter

    queen_1 = Graph.build_contiguity(tessellation, rook=False)
    tessellation["neighbors"] = momepy.neighbors(
        tessellation, queen_1, weighted=True
    )
    tessellation["covered_area"] = queen_1.describe(tessellation["area"])["sum"]
    buildings["neighbor_distance"] = momepy.neighbor_distance(buildings, queen_1)

    # Contiguity graphs can omit isolates; centroid graphs keep one node per building.
    adj_graph = Graph.build_triangulation(buildings.centroid)
    k_neighbors = min(15, max(1, len(buildings) - 1))
    neighborhood_graph = Graph.build_knn(buildings.centroid, k=k_neighbors)
    buildings["interbuilding_distance"] = momepy.mean_interbuilding_distance(
        buildings, adj_graph, neighborhood_graph
    )
    buildings_q1 = Graph.build_contiguity(buildings, rook=False)
    buildings["adjacency"] = momepy.building_adjacency(
        buildings_q1, neighborhood_graph
    )

    profile = momepy.street_profile(streets, buildings)
    streets["width"] = profile["width"].to_numpy()
    streets["width_deviation"] = profile["width_deviation"].to_numpy()
    streets["openness"] = profile["openness"].to_numpy()

    tessellation["car"] = buildings["area"] / tessellation["area"]

    nx_graph = momepy.gdf_to_nx(streets)
    nx_graph = momepy.node_degree(nx_graph)
    nx_graph = momepy.closeness_centrality(
        nx_graph, radius=400, distance="mm_len", verbose=False
    )
    nx_graph = momepy.meshedness(
        nx_graph, radius=400, distance="mm_len", verbose=False
    )
    nodes, streets = momepy.nx_to_gdf(nx_graph)

    nearest_edge = momepy.get_nearest_street(buildings, streets, max_distance=1000)
    buildings["nID"] = nearest_edge
    buildings["nodeID"] = momepy.get_nearest_node(
        buildings, nodes, streets, nearest_edge
    )
    tessellation["nID"] = buildings["nID"]

    # Avoid ambiguous index/column 'uID' during merges.
    buildings_attrs = buildings.drop(columns=["nID", "geometry"]).reset_index(drop=True)
    merged = tessellation.reset_index(drop=True).merge(buildings_attrs, on="uID")
    street_cols = streets.drop(columns="geometry", errors="ignore").copy()
    if "nID" not in street_cols.columns:
        street_cols["nID"] = street_cols.index
    merged = merged.merge(street_cols, on="nID", how="left", suffixes=("", "_street"))
    node_cols = nodes.drop(columns="geometry", errors="ignore")
    merged = merged.merge(node_cols, on="nodeID", how="left", suffixes=("", "_node"))
    return buildings, merged, queen_1


def _cluster_urban_types(
    buildings: gpd.GeoDataFrame,
    merged: gpd.GeoDataFrame,
    queen_1,
    nbr_cluster: int,
    momepy,
    Clustergram,
    plt,
) -> gpd.GeoDataFrame:
    """Percentile context features → Clustergram labels on buildings."""
    skip = {
        "uID",
        "nodeID",
        "nID",
        "mm_len",
        "node_start",
        "node_end",
        "geometry",
    }
    feature_cols = [
        c
        for c in merged.columns
        if c not in skip and pd.api.types.is_numeric_dtype(merged[c])
    ]

    # Percentiles need the same index as the contiguity graph (uID).
    merged_idx = merged.set_index("uID", drop=False)
    percentiles: list[pd.DataFrame] = []
    for column in feature_cols:
        try:
            perc = momepy.percentile(merged_idx[column], queen_1)
            perc.columns = [f"{column}_{x}" for x in perc.columns]
            percentiles.append(perc)
        except Exception as exc:  # noqa: BLE001 — momepy raises varied errors
            print(f"percentiles => {exc}")

    if not percentiles:
        raise RuntimeError("No percentile features could be computed for clustering.")

    percentiles_joined = pd.concat(percentiles, axis=1)
    standardized = (
        percentiles_joined - percentiles_joined.mean()
    ) / percentiles_joined.std()

    n_samples = len(standardized)
    max_k = min(11, n_samples)
    if nbr_cluster > max_k:
        raise ValueError(
            f"nbr_cluster={nbr_cluster} exceeds max feasible k={max_k} "
            f"for {n_samples} buildings."
        )
    cgram = Clustergram(range(1, max_k + 1), n_init=10, random_state=42)
    cgram.fit(standardized.fillna(0))
    cluster_labels = cgram.labels[nbr_cluster]
    cluster_labels.index = standardized.index

    urban_types = buildings[["geometry", "uID"]].copy()
    urban_types["cluster"] = urban_types["uID"].map(cluster_labels)
    unique_clusters = urban_types["cluster"].dropna().unique()
    color_map = {
        cluster: "#" + "".join(f"{int(c * 255):02x}" for c in plt.cm.tab10(i)[:3])
        for i, cluster in enumerate(unique_clusters)
    }
    urban_types["color"] = urban_types["cluster"].map(color_map)
    # uID is both index name and column — drop index for GPKG/Shapefile export.
    return urban_types.reset_index(drop=True)


class DetectionUrbanTypes:
    """Cluster buildings into urban morphotypes (momepy + Clustergram)."""

    def __init__(self, output_path: str | None = None) -> None:
        self.output_path = output_path if output_path else DEFAULT_TEMP_PATH
        self._bbox: list[float] | None = None
        self._epsg: int = DEFAULT_EPSG
        self.gdf: gpd.GeoDataFrame | None = None

    def set_bbox(
        self,
        min_x: float,
        min_y: float,
        max_x: float,
        max_y: float,
    ) -> None:
        """Set WGS84 bounding box (min_x, min_y, max_x, max_y)."""
        self._bbox = [min_x, min_y, max_x, max_y]

    def set_crs(self, epsg: int) -> None:
        """Set local projected CRS (EPSG code) used for morphometrics."""
        self._epsg = epsg

    @property
    def bbox(self) -> list[float] | None:
        return self._bbox

    @bbox.setter
    def bbox(self, value: list[float]) -> None:
        if len(value) != 4:
            raise ValueError("bbox must be [min_x, min_y, max_x, max_y]")
        self._bbox = list(value)

    @property
    def epsg(self) -> int:
        return self._epsg

    @epsg.setter
    def epsg(self, value: int) -> None:
        self._epsg = value

    def run(self, nbr_cluster: int = 4) -> Self:
        """Download OSM streets + IGN buildings, compute clusters, store gdf."""
        if self._bbox is None:
            raise ValueError("bbox is required; call set_bbox(...) first.")

        plt, momepy, osmnx, Clustergram, Graph = _require_urban_deps()
        local_crs = self._epsg
        bbox_final = _expand_bbox(self._bbox)
        envelope_polygon = box(*bbox_final)

        streets = _fetch_streets(envelope_polygon, local_crs, osmnx, momepy)
        buildings = _load_buildings(bbox_final, self.output_path, local_crs)
        buildings, merged, queen_1 = _compute_morphometrics(
            buildings, streets, momepy, Graph
        )
        self.gdf = _cluster_urban_types(
            buildings, merged, queen_1, nbr_cluster, momepy, Clustergram, plt
        )
        return self

    def to_gdf(self) -> gpd.GeoDataFrame:
        if self.gdf is None:
            raise RuntimeError("No result yet; call run() first.")
        return self.gdf

    def to_gpkg(self, name: str = "detection") -> str:
        """Write result GeoPackage; return output path."""
        if self.gdf is None:
            raise RuntimeError("No result yet; call run() first.")
        os.makedirs(self.output_path, exist_ok=True)
        path = os.path.join(self.output_path, f"{name}.gpkg")
        self.gdf.to_file(path, driver="GPKG")
        return path
