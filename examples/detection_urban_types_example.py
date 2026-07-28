"""Example: detect urban morphotype clusters (requires pymdurs[urban])."""

from __future__ import annotations

import os

from pymdurs.geometric import DetectionUrbanTypes


def main() -> None:
    output_path = os.environ.get("PYMDURS_OUTPUT", "./output")
    # La Rochelle sample bbox (WGS84), same spirit as Building examples
    bbox_wgs84 = (-1.152704, 46.181627, -1.139893, 46.18699)

    detection = DetectionUrbanTypes(output_path=output_path)
    detection.set_bbox(*bbox_wgs84)
    detection.set_crs(2154)
    detection = detection.run(nbr_cluster=4)

    gdf = detection.to_gdf()
    print(f"Clusters: {sorted(gdf['cluster'].unique())} ({len(gdf)} buildings)")
    out = detection.to_gpkg("detection_urban_types")
    print(f"Wrote {out}")


if __name__ == "__main__":
    main()
