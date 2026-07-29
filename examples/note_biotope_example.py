"""Example: Note de biotope — existant COSIA vs projet paysagiste.

Télécharge Cosia + Cadastre (parcelles) via IGN pour une bbox WGS84 ::

    python note_biotope_example.py
    python note_biotope_example.py --bbox minx,miny,maxx,maxy [projet.gpkg|shp]
    PYMDURS_BBOX=-1.15,46.18,-1.14,46.19 python note_biotope_example.py

Sans fichier projet, génère un scénario synthétique sur la parcelle.
Flag ``--synthetic`` : démo 100×100 m sans réseau.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

import geopandas as gpd
import matplotlib.pyplot as plt
import matplotlib.patches as mpatches
import numpy as np
import pandas as pd
import rasterio
from rasterio.features import shapes
from shapely.geometry import box, shape

import pymdurs
from pymdurs.indicators import TABLE_BFF, NoteBiotope
from pymdurs.geometric import TABLE_COLOR_COSIA

# La Rochelle (WGS84) — défaut exemples pymdurs
DEFAULT_BBOX_WGS84 = (-1.152704, 46.181627, -1.139893, 46.18699)
WORKING_CRS = 2154


def _figures_dir() -> Path:
    out = Path(os.environ.get("PYMDURS_OUTPUT", "./output")) / "note_biotope"
    out.mkdir(parents=True, exist_ok=True)
    return out


def _parse_bbox(raw: str) -> tuple[float, float, float, float]:
    parts = [float(x.strip()) for x in raw.split(",")]
    if len(parts) != 4:
        raise ValueError(f"Bbox attendue minx,miny,maxx,maxy — reçu: {raw!r}")
    return parts[0], parts[1], parts[2], parts[3]


def _hex_to_rgb(hex_color: str) -> tuple[int, int, int]:
    h = hex_color.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def vectorize_cosia_raster(cosia_tiff_path: str | Path) -> gpd.GeoDataFrame:
    """Vectorise le GeoTIFF COSIA (RGB → classe)."""
    rgb_to_class = {
        _hex_to_rgb(color): classe for classe, color in TABLE_COLOR_COSIA.items()
    }
    with rasterio.open(cosia_tiff_path) as src:
        image = src.read()
        transform = src.transform
        crs = src.crs
        combined = (
            (image[0].astype(np.uint32) << 16)
            + (image[1].astype(np.uint32) << 8)
            + image[2].astype(np.uint32)
        )
        geoms: list = []
        rgb_values: list[tuple[int, int, int]] = []
        for geom, value in shapes(combined, transform=transform):
            value_int = int(value)
            r = (value_int >> 16) & 255
            g = (value_int >> 8) & 255
            b = value_int & 255
            geoms.append(shape(geom))
            rgb_values.append((r, g, b))

    def match_color(rgb: tuple[int, int, int]) -> str:
        best, min_dist = "Autre", float("inf")
        for target_rgb, classe in rgb_to_class.items():
            dist = sum((a - b) ** 2 for a, b in zip(rgb, target_rgb))
            if dist < min_dist:
                min_dist, best = dist, classe
        return best

    gdf = gpd.GeoDataFrame({"rgb": rgb_values, "geometry": geoms}, crs=crs)
    gdf["classe"] = gdf["rgb"].apply(match_color)
    return gdf.drop(columns=["rgb"])


def fetch_parcelle(bbox_wgs84: tuple[float, float, float, float], output_path: Path) -> gpd.GeoDataFrame:
    """Télécharge le cadastre IGN (parcelles) pour la bbox."""
    print("⏳ Cadastre (parcelles) depuis IGN...")
    cadastre = pymdurs.geometric.Cadastre(output_path=str(output_path))
    cadastre.set_bbox(*bbox_wgs84)
    cadastre.set_crs(WORKING_CRS)
    cadastre = cadastre.run()
    geojson = cadastre.get_geojson()
    features = geojson.get("features", []) if isinstance(geojson, dict) else []
    if not features:
        raise RuntimeError("Aucune parcelle cadastre pour cette bbox.")
    gdf = gpd.GeoDataFrame.from_features(features, crs="EPSG:4326")
    print(f"  {len(gdf)} parcelles\n\n")
    return gdf.to_crs(epsg=WORKING_CRS)


def fetch_cosia(bbox_wgs84: tuple[float, float, float, float], output_path: Path) -> gpd.GeoDataFrame:
    """Télécharge COSIA IGN puis vectorise en polygones classés."""
    print("⏳ COSIA depuis IGN...")
    cosia = pymdurs.geometric.Cosia(output_path=str(output_path))
    cosia.set_bbox(*bbox_wgs84)
    cosia.set_crs(WORKING_CRS)
    cosia = cosia.run_ign()
    tiff_path = cosia.get_path_save_tiff()
    print(f"  Raster: {tiff_path}")
    gdf = vectorize_cosia_raster(tiff_path)
    print(f"  {len(gdf)} polygones COSIA\n\n")
    return gdf.to_crs(epsg=WORKING_CRS)


def generer_carte_occupation(gdf_m: gpd.GeoDataFrame, titre: str, nom_fichier: str) -> Path:
    """Carte colorée d'occupation du sol (classes COSIA ou BFF)."""
    fig, ax = plt.subplots(1, 1, figsize=(10, 10))
    color_col = "color" if "color" in gdf_m.columns else None
    if color_col:
        gdf_m.plot(ax=ax, color=gdf_m["color"], edgecolor="black", linewidth=0.3)
        classes = gdf_m["classe"].unique() if "classe" in gdf_m.columns else []
        patches = [
            mpatches.Patch(
                facecolor=TABLE_COLOR_COSIA.get(c, "#888888"),
                edgecolor="black",
                linewidth=0.5,
                label=c,
            )
            for c in sorted(classes)
        ]
        legend_title = "Classes COSIA"
    else:
        cmap = plt.get_cmap("YlGn")
        gdf_m.plot(
            ax=ax,
            column="coef",
            cmap=cmap,
            vmin=0,
            vmax=1,
            edgecolor="black",
            linewidth=0.3,
        )
        patches = [
            mpatches.Patch(
                facecolor=cmap(c),
                edgecolor="black",
                linewidth=0.5,
                label=f"{k} ({c})",
            )
            for k, c in sorted(
                {row.bff_class: row.coef for row in gdf_m.itertuples()}.items(),
                key=lambda x: -x[1],
            )
        ]
        legend_title = "Classes BFF"

    ax.set_title(titre, fontsize=14)
    ax.set_axis_off()
    if patches:
        ax.legend(
            handles=patches,
            loc="lower left",
            fontsize=8,
            framealpha=0.9,
            title=legend_title,
            title_fontsize=9,
        )
    plt.tight_layout()
    fig_dir = _figures_dir()
    fig_path = fig_dir / f"{nom_fichier}.png"
    plt.savefig(fig_path, dpi=150, bbox_inches="tight")
    plt.savefig(fig_dir / f"{nom_fichier}.pdf", dpi=150, bbox_inches="tight")
    plt.close()
    print(f"  Carte sauvegardée : {fig_path}")
    return fig_path


def generer_barplot_occupation(df_surface: pd.DataFrame, titre: str, nom_fichier: str) -> Path:
    """Diagramme à barres (part de surface % ou surface pondérée)."""
    plt.figure(figsize=(11, 6))
    label_col = "classe" if "classe" in df_surface.columns else "bff_class"
    value_col = "surface_pct" if "surface_pct" in df_surface.columns else "surface_m2"
    colors = (
        df_surface["color"]
        if "color" in df_surface.columns
        else ["#8cd76a"] * len(df_surface)
    )
    bars = plt.bar(df_surface[label_col], df_surface[value_col], color=colors, edgecolor="black")
    plt.xlabel(label_col)
    plt.ylabel("Part de surface (%)" if value_col == "surface_pct" else "Surface (m²)")
    plt.title(titre)
    plt.xticks(rotation=30, ha="right")
    plt.grid(axis="y", linestyle="--", alpha=0.5)
    for bar in bars:
        height = bar.get_height()
        plt.text(
            bar.get_x() + bar.get_width() / 2,
            height,
            f"{height:.1f}",
            ha="center",
            va="bottom",
            fontsize=9,
        )
    plt.tight_layout()
    fig_dir = _figures_dir()
    fig_path = fig_dir / f"{nom_fichier}.png"
    plt.savefig(fig_path, dpi=150, bbox_inches="tight")
    plt.close()
    print(f"  Barplot sauvegardé : {fig_path}")
    return fig_path


def _detail_to_bar_df(detail: pd.DataFrame, surface_totale: float) -> pd.DataFrame:
    df = detail.copy()
    if df.empty:
        return df
    df["surface_totale_bff"] = df["surface_m2"] + df["credit_arbre_m2"]
    df["surface_pct"] = 100.0 * df["surface_totale_bff"] / surface_totale
    df["color"] = df["coef"].map(lambda c: plt.get_cmap("YlGn")(float(c)))
    return df


def _synthetic_projet(parcelle: gpd.GeoDataFrame) -> gpd.GeoDataFrame:
    """Scénario projet démo : 40 % pleine terre, 60 % imperméable."""
    geom = parcelle.dissolve().geometry.iloc[0]
    minx, miny, maxx, maxy = geom.bounds
    mid = minx + 0.4 * (maxx - minx)
    return gpd.GeoDataFrame(
        {
            "bff_class": [
                "vegetation_pleine_terre_contact_direct_sol",
                "surface_totalement_impermeable_sans_vegetation",
            ],
        },
        geometry=[
            box(minx, miny, mid, maxy),
            box(mid, miny, maxx, maxy),
        ],
        crs=parcelle.crs,
    )


def main() -> None:
    args = list(sys.argv[1:])
    if "--synthetic" in args:
        args.remove("--synthetic")
        print("Mode --synthetic — démo 100×100 m sans réseau.")
        parcelle = gpd.GeoDataFrame(geometry=[box(0, 0, 100, 100)], crs="EPSG:2154")
        cosia = gpd.GeoDataFrame(
            {"classe": ["Pelouse", "Zone imperméable"]},
            geometry=[box(0, 0, 60, 100), box(60, 0, 100, 100)],
            crs="EPSG:2154",
        )
        projet = _synthetic_projet(parcelle)
        _run(parcelle, cosia, projet)
        return

    bbox_raw: str | None = None
    if "--bbox" in args:
        i = args.index("--bbox")
        if i + 1 >= len(args):
            raise SystemExit("Usage: --bbox minx,miny,maxx,maxy")
        bbox_raw = args.pop(i + 1)
        args.pop(i)
    elif os.environ.get("PYMDURS_BBOX"):
        bbox_raw = os.environ["PYMDURS_BBOX"]

    bbox = _parse_bbox(bbox_raw) if bbox_raw else DEFAULT_BBOX_WGS84
    output_path = _figures_dir().parent
    output_path.mkdir(parents=True, exist_ok=True)

    print(f"Bbox WGS84: {bbox}")
    parcelle = fetch_parcelle(bbox, output_path)
    cosia = fetch_cosia(bbox, output_path)

    projet_path = args[0] if args else os.environ.get("PYMDURS_PROJET", "")
    if projet_path and Path(projet_path).exists():
        projet = gpd.read_file(projet_path)
    else:
        projet = _synthetic_projet(parcelle)

    _run(parcelle, cosia, projet)


def _run(parcelle: gpd.GeoDataFrame, cosia: gpd.GeoDataFrame, projet: gpd.GeoDataFrame) -> None:
    print("=" * 70)
    print("  NOTE DE BIOTOPE — COSIA (existant) vs projet")
    print("=" * 70)

    nb = NoteBiotope(parcelle).load_existant_cosia(cosia).load_projet_gdf(projet)
    result = nb.compute()

    print(f"  Surface parcelle : {result.surface_parcelle_m2:.0f} m²")
    print(f"  CBS existant     : {result.cbs_existant:.3f}" if result.cbs_existant is not None else "")
    print(f"  CBS projet       : {result.cbs_projet:.3f}" if result.cbs_projet is not None else "")
    if result.delta is not None:
        print(f"  Delta            : {result.delta:+.3f}")
    if result.alerte_diminution:
        print("  ALERTE : diminution de potentiel biotope (projet < existant)")

    existant = nb.existant_gdf
    if existant is not None and not existant.empty:
        out_dir = _figures_dir()
        shp_existant = out_dir / "cosia_existant.shp"
        existant.to_file(shp_existant, driver="ESRI Shapefile")
        print(f"  Shapefile COSIA existant : {shp_existant}")

        geojson_existant = out_dir / "cosia_existant.geojson"
        # Propriétés COSIA (classe, couleur) + BFF annotés
        cols = [
            c
            for c in (
                "classe",
                "color",
                "couleur",
                "bff_class",
                "coef",
                "surface_m2",
                "credit_arbre_m2",
            )
            if c in existant.columns
        ]
        existant[cols + ["geometry"]].to_file(geojson_existant, driver="GeoJSON")
        print(f"  GeoJSON COSIA existant   : {geojson_existant}")

        df = existant.copy()
        df["surface_m2"] = df.geometry.area
        by_classe: pd.DataFrame = pd.DataFrame(
            df.groupby(["classe", "color"], as_index=False).agg(surface_m2=("surface_m2", "sum"))
            if "classe" in df.columns
            else {}
        )
        if not by_classe.empty:
            total = by_classe["surface_m2"].sum()
            by_classe["surface_pct"] = 100 * by_classe["surface_m2"] / total
            generer_carte_occupation(existant, "Occupation du sol — État existant (COSIA)", "carte_cosia_existant")
            generer_barplot_occupation(
                by_classe, "Répartition surfacique — État existant (COSIA)", "occupation_sol_existant"
            )

    projet_gdf = nb.projet_gdf
    if projet_gdf is not None and not projet_gdf.empty:
        shp_projet = _figures_dir() / "projet_bff.shp"
        projet_gdf.to_file(shp_projet, driver="ESRI Shapefile")
        print(f"  Shapefile projet BFF     : {shp_projet}")

        generer_carte_occupation(projet_gdf, "Occupation du sol — Projet (BFF)", "carte_projet_bff")
        bar_df = _detail_to_bar_df(result.detail_projet, result.surface_parcelle_m2)
        if not bar_df.empty:
            generer_barplot_occupation(bar_df, "Répartition BFF — Projet", "occupation_sol_projet")

    print("  Clés BFF disponibles :", len(TABLE_BFF))
    print("  Sorties :", _figures_dir())


if __name__ == "__main__":
    main()
