"""Note de biotope (CBS / BFF) : existant COSIA vs projet paysager.

CBS = sum(S_i * c_i) / S_parcelle.

Les forfaits arbres (UV-5 types 3 et 10) ajoutent ``n_arbres * forfait_m2``
au numérateur (crédit réglementaire), sans remplacer la surface sol sous-jacente.
"""

from __future__ import annotations

import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Self

import geopandas as gpd
import pandas as pd

from ..geometric_helpers.cosia_colors import TABLE_COLOR_COSIA
from ..geometric_helpers.dxf2shp import dxf_to_polygon_shp

DEFAULT_CRS = "EPSG:2154"

# ---------------------------------------------------------------------------
# Coefficients BFF (source de vérité pour le score)
# ---------------------------------------------------------------------------

TABLE_BFF: dict[str, float] = {
    "surface_totalement_impermeable_sans_vegetation": 0.0,
    "surface_partiellement_permeable_sans_vegetation": 0.1,
    "surface_tres_permeable_sans_vegetation": 0.2,
    "surface_pavee_permeable_avec_vegetation_permanente": 0.4,
    "vegetation_hors_sol_substrat_20_40cm": 0.5,
    "vegetation_hors_sol_substrat_41_80cm": 0.6,
    "vegetation_hors_sol_substrat_81_150cm": 0.7,
    "vegetation_hors_sol_substrat_plus_150cm": 0.9,
    "vegetation_pleine_terre_contact_direct_sol": 1.0,
    "zone_infiltration_eaux_pluviales_surface": 0.2,
    "surface_eau_alimentee_par_eaux_pluviales": 0.5,
    "toiture_vegetalisee_extensive_substrat_moins_20cm": 0.5,
    "toiture_vegetalisee_simple_intensive_substrat_15_50cm": 0.7,
    "toiture_vegetalisee_intensive_substrat_plus_50cm": 0.8,
    "vegetalisation_facade_plantes_enracinees_sol_jusqu_10m": 0.5,
    "mur_vegetal_sans_contact_sol_avec_irrigation": 0.7,
}

# ---------------------------------------------------------------------------
# COSIA → BFF (état existant)
# ---------------------------------------------------------------------------

COSIA_TO_BFF: dict[str, str] = {
    "Bâtiment": "surface_totalement_impermeable_sans_vegetation",
    "Zone imperméable": "surface_totalement_impermeable_sans_vegetation",
    "Serre": "surface_totalement_impermeable_sans_vegetation",
    "Autre": "surface_totalement_impermeable_sans_vegetation",
    "Neige": "surface_totalement_impermeable_sans_vegetation",
    "Zone perméable": "surface_tres_permeable_sans_vegetation",
    "Sol nu": "surface_tres_permeable_sans_vegetation",
    "Terre labourée": "surface_tres_permeable_sans_vegetation",
    "Pelouse": "vegetation_pleine_terre_contact_direct_sol",
    "Broussaille": "vegetation_pleine_terre_contact_direct_sol",
    "Culture": "vegetation_pleine_terre_contact_direct_sol",
    "Vigne": "vegetation_pleine_terre_contact_direct_sol",
    "Coupe": "vegetation_pleine_terre_contact_direct_sol",
    "Conifère": "vegetation_pleine_terre_contact_direct_sol",
    "Feuillu": "vegetation_pleine_terre_contact_direct_sol",
    "Surface eau": "surface_eau_alimentee_par_eaux_pluviales",
    "Piscine": "surface_eau_alimentee_par_eaux_pluviales",
}

# ---------------------------------------------------------------------------
# PLUi UV-5 type → BFF (vocabulaire paysagiste)
# ---------------------------------------------------------------------------

PLUI_TO_BFF: dict[int, str] = {
    1: "vegetation_pleine_terre_contact_direct_sol",
    2: "vegetation_pleine_terre_contact_direct_sol",
    3: "vegetation_pleine_terre_contact_direct_sol",  # forfait arbres
    4: "vegetation_hors_sol_substrat_81_150cm",
    5: "vegetation_hors_sol_substrat_20_40cm",
    6: "toiture_vegetalisee_extensive_substrat_moins_20cm",
    7: "zone_infiltration_eaux_pluviales_surface",
    8: "surface_pavee_permeable_avec_vegetation_permanente",
    9: "vegetalisation_facade_plantes_enracinees_sol_jusqu_10m",
    10: "vegetation_pleine_terre_contact_direct_sol",  # forfait arbres
}

# Forfaits UV-5 (m² par arbre) : type 3 moyen/grand, type 10 planté.
FORFAIT_ARBRE_TYPE3_MOYEN_M2 = 20.0
FORFAIT_ARBRE_TYPE3_GRAND_M2 = 50.0
FORFAIT_ARBRE_TYPE10_M2 = 10.0

# ---------------------------------------------------------------------------
# Calques ENON → BFF (+ métadonnées forfait arbre)
# ---------------------------------------------------------------------------

# value: bff_class | dict with bff_class, n_arbres_per_feature, forfait_arbre_m2
LAYER_TO_BFF: dict[str, str | dict[str, float | str | int]] = {
    "ENON-COUPES": "surface_totalement_impermeable_sans_vegetation",
    "ENON-arbres 12-14": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres 25-30": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres TRB 12-14": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres existants": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE3_MOYEN_M2,
    },
    "ENON-arbres multitroncs": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres multitroncs 200-250": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE3_GRAND_M2,
    },
    "ENON-arbres tige 12-14": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres tige 25-30": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-arbres tige fruitiers 12-14": {
        "bff_class": "vegetation_pleine_terre_contact_direct_sol",
        "n_arbres_per_feature": 1,
        "forfait_arbre_m2": FORFAIT_ARBRE_TYPE10_M2,
    },
    "ENON-boules de granite": "surface_totalement_impermeable_sans_vegetation",
    "ENON-clôtures bois opaques": "surface_totalement_impermeable_sans_vegetation",
    "ENON-copeaux bois Jeux": "surface_partiellement_permeable_sans_vegetation",
    "ENON-cotations": "surface_totalement_impermeable_sans_vegetation",
    "ENON-couvre-sol massif": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-couvre-sol pied d'arbre": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-engazonnement": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-fonte": "surface_totalement_impermeable_sans_vegetation",
    "ENON-jeux": "surface_totalement_impermeable_sans_vegetation",
    "ENON-massif arbustif bas": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-massifs arbustifs hauts": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-massifs boisés micro-forêts": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-minéral": "surface_totalement_impermeable_sans_vegetation",
    "ENON-mobilier bois": "surface_totalement_impermeable_sans_vegetation",
    "ENON-pavés joints enherbés": "surface_pavee_permeable_avec_vegetation_permanente",
    "ENON-plantations existantes": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-plantations existantes à compléter": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-ponton estrade terrasse bois": "surface_totalement_impermeable_sans_vegetation",
    "ENON-potager (terre végétale)": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-sol vert": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-vert": "vegetation_pleine_terre_contact_direct_sol",
    "ENON-école sols souple jeux": "surface_totalement_impermeable_sans_vegetation",
    "MUR": "surface_totalement_impermeable_sans_vegetation",
}


def bff_coef_from_epaisseur_m(epaisseur_m: float, *, toiture: bool = False) -> str:
    """Map substrate thickness (m) to a BFF class key (hors-sol / toiture)."""
    cm = epaisseur_m * 100.0
    if toiture:
        if cm < 20.0:
            return "toiture_vegetalisee_extensive_substrat_moins_20cm"
        if cm <= 50.0:
            return "toiture_vegetalisee_simple_intensive_substrat_15_50cm"
        return "toiture_vegetalisee_intensive_substrat_plus_50cm"
    if cm < 20.0:
        return "toiture_vegetalisee_extensive_substrat_moins_20cm"
    if cm <= 40.0:
        return "vegetation_hors_sol_substrat_20_40cm"
    if cm <= 80.0:
        return "vegetation_hors_sol_substrat_41_80cm"
    if cm <= 150.0:
        return "vegetation_hors_sol_substrat_81_150cm"
    return "vegetation_hors_sol_substrat_plus_150cm"


def _read_gdf(source: gpd.GeoDataFrame | Path | str) -> gpd.GeoDataFrame:
    if isinstance(source, gpd.GeoDataFrame):
        return source.copy()
    path = Path(source)
    if not path.exists():
        raise FileNotFoundError(f"GeoDataFrame file not found: {path}")
    return gpd.read_file(path)


def _clip_to_parcelle(
    gdf: gpd.GeoDataFrame,
    parcelle: gpd.GeoDataFrame,
    crs: str,
) -> gpd.GeoDataFrame:
    """Clip ``gdf`` to dissolved parcel, reproject to ``crs``."""
    if gdf.crs is None:
        raise ValueError("Input GeoDataFrame has no CRS.")
    if parcelle.crs is None:
        raise ValueError("Parcelle GeoDataFrame has no CRS.")

    parcelle_m = parcelle.to_crs(crs)
    parcelle_diss = parcelle_m.dissolve()
    gdf_aligned = gdf.to_crs(crs)
    clipped = gpd.GeoDataFrame(gpd.clip(gdf_aligned, parcelle_diss), crs=crs)
    mask = clipped.geometry.notnull() & ~clipped.geometry.is_empty
    clipped = gpd.GeoDataFrame(clipped.loc[mask], crs=crs)
    if clipped.empty:
        return clipped
    return gpd.GeoDataFrame(clipped.loc[clipped.is_valid].copy(), crs=crs)


def _layer_meta(layer_name: object, layer_to_bff: dict) -> dict[str, float | str | int]:
    """Normalize LAYER_TO_BFF entry to a dict with at least ``bff_class``."""
    raw = layer_to_bff.get(layer_name) if isinstance(layer_name, str) else None
    if raw is None:
        return {"bff_class": "surface_totalement_impermeable_sans_vegetation"}
    if isinstance(raw, str):
        return {"bff_class": raw}
    return dict(raw)


def _row_value(row: pd.Series, column: str) -> object | None:
    """Return a scalar cell value, or None if missing / NA."""
    if column not in row.index:
        return None
    value = row[column]
    if value is None or (isinstance(value, float) and pd.isna(value)):
        return None
    try:
        if bool(pd.isna(value)):
            return None
    except (TypeError, ValueError):
        pass
    return value


def resolve_bff_class(
    *,
    bff_class: str | None = None,
    type_plui: int | None = None,
    epaisseur_m: float | None = None,
    toiture: bool = False,
    cosia_classe: str | None = None,
    layer: str | None = None,
    layer_to_bff: dict | None = None,
) -> str:
    """Resolve BFF class with priority: bff_class > epaisseur > type_plui > layer > COSIA."""
    if bff_class is not None:
        if bff_class not in TABLE_BFF:
            raise ValueError(f"Unknown bff_class: {bff_class!r}")
        return bff_class
    if epaisseur_m is not None:
        return bff_coef_from_epaisseur_m(float(epaisseur_m), toiture=toiture)
    if type_plui is not None:
        key = PLUI_TO_BFF.get(int(type_plui))
        if key is None:
            raise ValueError(f"Unknown type_plui: {type_plui!r}")
        return key
    if layer is not None:
        meta = _layer_meta(layer, layer_to_bff or LAYER_TO_BFF)
        return str(meta["bff_class"])
    if cosia_classe is not None:
        key = COSIA_TO_BFF.get(cosia_classe)
        if key is None:
            raise ValueError(f"Unknown COSIA class: {cosia_classe!r}")
        return key
    raise ValueError("Cannot resolve bff_class: no attribute provided.")


def _annotate_bff(
    gdf: gpd.GeoDataFrame,
    *,
    layer_to_bff: dict | None = None,
) -> gpd.GeoDataFrame:
    """Add ``bff_class``, ``coef``, ``surface_m2``, optional tree credit columns."""
    out = gdf.copy()
    props = layer_to_bff if layer_to_bff is not None else LAYER_TO_BFF

    bff_classes: list[str] = []
    credits: list[float] = []

    for _, row in out.iterrows():
        bff_attr = _row_value(row, "bff_class")
        type_raw = _row_value(row, "type_plui")
        type_plui = int(type_raw) if type_raw is not None else None  # type: ignore[arg-type]
        ep_raw = _row_value(row, "epaisseur_m")
        epaisseur = float(ep_raw) if ep_raw is not None else None  # type: ignore[arg-type]
        toiture_raw = _row_value(row, "toiture")
        toiture = bool(toiture_raw) if toiture_raw is not None else False
        cosia = _row_value(row, "classe")
        layer = _row_value(row, "Layer")

        bff = resolve_bff_class(
            bff_class=str(bff_attr) if bff_attr is not None else None,
            type_plui=type_plui,
            epaisseur_m=epaisseur,
            toiture=toiture,
            cosia_classe=str(cosia) if cosia is not None else None,
            layer=str(layer) if layer is not None else None,
            layer_to_bff=props,
        )
        bff_classes.append(bff)

        credit = 0.0
        forfait = _row_value(row, "forfait_arbre_m2")
        n_arbres = _row_value(row, "n_arbres")
        if forfait is not None and n_arbres is not None:
            credit = float(n_arbres) * float(forfait)  # type: ignore[arg-type]
        elif layer is not None:
            meta = _layer_meta(layer, props)
            if "forfait_arbre_m2" in meta:
                n = float(meta.get("n_arbres_per_feature", 1))
                if n_arbres is not None:
                    n = float(n_arbres)  # type: ignore[arg-type]
                credit = n * float(meta["forfait_arbre_m2"])
        credits.append(credit)

    out["bff_class"] = bff_classes
    out["coef"] = [TABLE_BFF[c] for c in bff_classes]
    out["surface_m2"] = out.geometry.area
    out["credit_arbre_m2"] = credits
    # Tree layers: geometric footprint is not the BFF surface; credit is.
    is_tree_credit = out["credit_arbre_m2"] > 0
    out.loc[is_tree_credit, "surface_m2"] = 0.0
    return out


def _surfaces_by_bff(gdf: gpd.GeoDataFrame) -> pd.DataFrame:
    """Aggregate geometric surfaces and tree credits by BFF class."""
    if gdf.empty:
        return pd.DataFrame(
            columns=["bff_class", "coef", "surface_m2", "credit_arbre_m2", "surface_ponderee"]
        )

    geom = pd.DataFrame(
        gdf.groupby("bff_class", as_index=False).agg(
            surface_m2=("surface_m2", "sum"),
            credit_arbre_m2=("credit_arbre_m2", "sum"),
        )
    )
    geom["coef"] = geom["bff_class"].map(lambda c: TABLE_BFF[str(c)])
    geom["surface_ponderee"] = (geom["surface_m2"] + geom["credit_arbre_m2"]) * geom["coef"]
    return geom


def _cbs_from_detail(detail: pd.DataFrame, surface_parcelle_m2: float) -> float:
    if surface_parcelle_m2 <= 0:
        raise ValueError(f"Parcel area must be > 0, got {surface_parcelle_m2}")
    if detail.empty:
        return 0.0
    return float(detail["surface_ponderee"].sum() / surface_parcelle_m2)


@dataclass(frozen=True)
class BiotopeResult:
    """Résultat du calcul de note de biotope."""

    cbs_existant: float | None
    cbs_projet: float | None
    delta: float | None
    alerte_diminution: bool
    surface_parcelle_m2: float
    detail_existant: pd.DataFrame
    detail_projet: pd.DataFrame


class NoteBiotope:
    """Calcule le CBS existant (COSIA) et projet (DXF / GDF paysagiste)."""

    def __init__(self, parcelle: gpd.GeoDataFrame, crs: str = DEFAULT_CRS) -> None:
        if parcelle.empty:
            raise ValueError("parcelle GeoDataFrame is empty.")
        self.crs = crs
        self.parcelle = parcelle.to_crs(crs) if parcelle.crs else parcelle.set_crs(crs)
        self.surface_parcelle_m2 = float(self.parcelle.dissolve().geometry.area.iloc[0])
        if self.surface_parcelle_m2 <= 0:
            raise ValueError("Parcel area must be > 0.")

        self._existant: gpd.GeoDataFrame | None = None
        self._projet: gpd.GeoDataFrame | None = None
        self._detail_existant = pd.DataFrame()
        self._detail_projet = pd.DataFrame()

    def load_existant_cosia(self, cosia: gpd.GeoDataFrame | Path | str) -> Self:
        """Clip COSIA ∩ parcelle, map ``classe`` → BFF, store surfaces."""
        gdf = _read_gdf(cosia)
        if "classe" not in gdf.columns:
            raise KeyError("COSIA GeoDataFrame must contain a 'classe' column.")
        clipped = _clip_to_parcelle(gdf, self.parcelle, self.crs)
        self._existant = _annotate_bff(clipped)
        if "classe" in self._existant.columns:
            self._existant["color"] = self._existant["classe"].map(
                lambda c: TABLE_COLOR_COSIA.get(c, "#888888")
            )
        self._detail_existant = _surfaces_by_bff(self._existant)
        return self

    def load_projet_dxf(
        self,
        dxf: Path | str,
        *,
        layer_to_bff: dict | None = None,
        encoding: str = "UTF-8",
        dxf_crs: str = "EPSG:3946",
    ) -> Self:
        """Convert DXF → polygons, assign BFF via ``LAYER_TO_BFF`` (or override)."""
        dxf_path = Path(dxf)
        if not dxf_path.exists():
            raise FileNotFoundError(f"DXF not found: {dxf_path}")

        with tempfile.TemporaryDirectory(prefix="pymdurs_biotope_") as tmp:
            shp = Path(tmp) / "dxf_polygons.shp"
            gdf = dxf_to_polygon_shp(dxf_path, shp, encoding=encoding, crs=dxf_crs)

        if gdf.crs is None:
            gdf = gdf.set_crs(dxf_crs)
        clipped = _clip_to_parcelle(gdf, self.parcelle, self.crs)
        props = layer_to_bff if layer_to_bff is not None else LAYER_TO_BFF
        if "Layer" in clipped.columns:
            known = clipped["Layer"].isin(list(props.keys()))
            clipped = gpd.GeoDataFrame(clipped.loc[known].copy(), crs=self.crs)
        self._projet = _annotate_bff(clipped, layer_to_bff=props)
        self._detail_projet = _surfaces_by_bff(self._projet)
        return self

    def load_projet_gdf(
        self,
        gdf: gpd.GeoDataFrame | Path | str,
        *,
        layer_to_bff: dict | None = None,
    ) -> Self:
        """Load an already attributed project GeoDataFrame (paysagiste).

        Accepted attributes (priority in ``resolve_bff_class``):
        ``bff_class``, ``epaisseur_m`` (+ optional ``toiture``), ``type_plui``,
        ``Layer``, ``classe``. Optional tree credit: ``n_arbres``, ``forfait_arbre_m2``.
        """
        source = _read_gdf(gdf)
        clipped = _clip_to_parcelle(source, self.parcelle, self.crs)
        self._projet = _annotate_bff(clipped, layer_to_bff=layer_to_bff)
        self._detail_projet = _surfaces_by_bff(self._projet)
        return self

    @property
    def existant_gdf(self) -> gpd.GeoDataFrame | None:
        return self._existant

    @property
    def projet_gdf(self) -> gpd.GeoDataFrame | None:
        return self._projet

    def compute(self) -> BiotopeResult:
        """Return CBS existant / projet, delta, and diminution alert."""
        cbs_e = (
            _cbs_from_detail(self._detail_existant, self.surface_parcelle_m2)
            if self._existant is not None
            else None
        )
        cbs_p = (
            _cbs_from_detail(self._detail_projet, self.surface_parcelle_m2)
            if self._projet is not None
            else None
        )
        delta = None
        alerte = False
        if cbs_e is not None and cbs_p is not None:
            delta = cbs_p - cbs_e
            alerte = cbs_p < cbs_e

        return BiotopeResult(
            cbs_existant=cbs_e,
            cbs_projet=cbs_p,
            delta=delta,
            alerte_diminution=alerte,
            surface_parcelle_m2=self.surface_parcelle_m2,
            detail_existant=self._detail_existant.copy(),
            detail_projet=self._detail_projet.copy(),
        )


__all__ = [
    "TABLE_BFF",
    "COSIA_TO_BFF",
    "PLUI_TO_BFF",
    "LAYER_TO_BFF",
    "FORFAIT_ARBRE_TYPE3_MOYEN_M2",
    "FORFAIT_ARBRE_TYPE3_GRAND_M2",
    "FORFAIT_ARBRE_TYPE10_M2",
    "BiotopeResult",
    "NoteBiotope",
    "bff_coef_from_epaisseur_m",
    "resolve_bff_class",
]
