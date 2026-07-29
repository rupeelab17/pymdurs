"""Unit tests for NoteBiotope (synthetic geometries, no network)."""

from __future__ import annotations

import geopandas as gpd
import pytest
from shapely.geometry import Point, box

from pymdurs.indicators import (
    COSIA_TO_BFF,
    FORFAIT_ARBRE_TYPE10_M2,
    TABLE_BFF,
    NoteBiotope,
    bff_coef_from_epaisseur_m,
    resolve_bff_class,
)

CRS = "EPSG:2154"


def _parcelle(xmin: float = 0.0, ymin: float = 0.0, side: float = 100.0) -> gpd.GeoDataFrame:
    return gpd.GeoDataFrame({"id": [1]}, geometry=[box(xmin, ymin, xmin + side, ymin + side)], crs=CRS)


def _cosia_half_half() -> gpd.GeoDataFrame:
    """100×100 parcelle: left Pelouse, right Zone imperméable."""
    return gpd.GeoDataFrame(
        {
            "classe": ["Pelouse", "Zone imperméable"],
        },
        geometry=[box(0, 0, 50, 100), box(50, 0, 100, 100)],
        crs=CRS,
    )


def test_resolve_bff_priority():
    assert resolve_bff_class(bff_class="zone_infiltration_eaux_pluviales_surface") == (
        "zone_infiltration_eaux_pluviales_surface"
    )
    assert resolve_bff_class(epaisseur_m=0.9) == "vegetation_hors_sol_substrat_81_150cm"
    assert resolve_bff_class(type_plui=8) == "surface_pavee_permeable_avec_vegetation_permanente"
    assert resolve_bff_class(cosia_classe="Feuillu") == COSIA_TO_BFF["Feuillu"]


def test_bff_coef_from_epaisseur():
    assert bff_coef_from_epaisseur_m(0.15) == "toiture_vegetalisee_extensive_substrat_moins_20cm"
    assert bff_coef_from_epaisseur_m(0.3) == "vegetation_hors_sol_substrat_20_40cm"
    assert bff_coef_from_epaisseur_m(0.6, toiture=True) == (
        "toiture_vegetalisee_intensive_substrat_plus_50cm"
    )


def test_cbs_existant_half_pelouse():
    nb = NoteBiotope(_parcelle()).load_existant_cosia(_cosia_half_half())
    result = nb.compute()
    assert result.surface_parcelle_m2 == pytest.approx(10_000.0)
    # 5000 * 1.0 + 5000 * 0.0 / 10000 = 0.5
    assert result.cbs_existant == pytest.approx(0.5)
    assert result.cbs_projet is None
    assert result.alerte_diminution is False


def test_alerte_diminution_projet_inferieur():
    parcelle = _parcelle()
    cosia = _cosia_half_half()
    # Projet entièrement imperméable → CBS 0 < 0.5
    projet = gpd.GeoDataFrame(
        {"bff_class": ["surface_totalement_impermeable_sans_vegetation"]},
        geometry=[box(0, 0, 100, 100)],
        crs=CRS,
    )
    result = (
        NoteBiotope(parcelle)
        .load_existant_cosia(cosia)
        .load_projet_gdf(projet)
        .compute()
    )
    assert result.cbs_existant == pytest.approx(0.5)
    assert result.cbs_projet == pytest.approx(0.0)
    assert result.delta == pytest.approx(-0.5)
    assert result.alerte_diminution is True


def test_projet_override_type_plui():
    parcelle = _parcelle()
    projet = gpd.GeoDataFrame(
        {"type_plui": [1]},
        geometry=[box(0, 0, 100, 100)],
        crs=CRS,
    )
    result = NoteBiotope(parcelle).load_projet_gdf(projet).compute()
    assert result.cbs_projet == pytest.approx(1.0)


def test_forfait_arbre_ajoute_numerateur():
    """Tree credit adds forfait×coef to numerator; geometric footprint ignored."""
    parcelle = _parcelle()  # 10_000 m²
    # Imperméable everywhere + 2 planted trees × 10 m² × coef 1.0
    projet = gpd.GeoDataFrame(
        {
            "bff_class": [
                "surface_totalement_impermeable_sans_vegetation",
                "vegetation_pleine_terre_contact_direct_sol",
                "vegetation_pleine_terre_contact_direct_sol",
            ],
            "n_arbres": [None, 1, 1],
            "forfait_arbre_m2": [None, FORFAIT_ARBRE_TYPE10_M2, FORFAIT_ARBRE_TYPE10_M2],
        },
        geometry=[
            box(0, 0, 100, 100),
            Point(10, 10).buffer(2),
            Point(30, 30).buffer(2),
        ],
        crs=CRS,
    )
    result = NoteBiotope(parcelle).load_projet_gdf(projet).compute()
    # (0 + 10 + 10) / 10000 = 0.002
    assert result.cbs_projet == pytest.approx(20.0 / 10_000.0)


def test_all_cosia_classes_mapped():
    for classe in COSIA_TO_BFF:
        key = COSIA_TO_BFF[classe]
        assert key in TABLE_BFF
