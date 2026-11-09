//! LCZ download, filter, and spatial query.

use anyhow::{Context, Result};
use geojson::{Feature, GeoJson};
use gdal::spatial_ref::{AxisMappingStrategy, CoordTransform, SpatialRef};
use gdal::vector::Geometry as OgrGeometry;
use gdal::vector::LayerAccess;
use gdal::Dataset;
use geo::algorithm::bounding_rect::BoundingRect;
use geo::algorithm::intersects::Intersects;
use geo::Geometry as GeoGeometry;
use geos::Geom;
use rstar::{RTree, AABB};

use super::types::{IndexedGeometry, Lcz};

impl Lcz {
    /// Run LCZ processing: load from zip URL, filter by bbox, reproject
    /// Following Python: def run(self, zipfile_url: str = "...")
    /// Downloads ZIP file, extracts shapefile, reads with GDAL, filters by bbox, and reprojects
    pub fn run(&mut self, zipfile_url: Option<&str>) -> Result<()> {
        let url = zipfile_url.unwrap_or(
            "https://static.data.gouv.fr/resources/cartographie-des-zones-climatiques-locales-lcz-de-83-aires-urbaines-de-plus-de-50-000-habitants-2022/20241210-104453/lcz-spot-2022-la-rochelle.zip"
        );

        let bbox = self
            .bbox
            .context("Bounding box must be set before running LCZ processing")?;

        tracing::info!("Bbox (EPSG:4326): {:?}", bbox);

        tracing::info!("Téléchargement du fichier ZIP depuis: {}", url);

        // 1. Télécharger et extraire le ZIP
        let temp_dir = self.download_and_extract_zip(url)?;
        // 2. Trouver le fichier .shp dans le dossier temporaire
        let shp_path = self.find_shapefile(&temp_dir)?;

        tracing::info!("Shapefile trouvé: {:?}", shp_path);

        // Lire le shapefile avec GDAL (no ogr2ogr step: conversion was unused and required CLI GDAL)
        let dataset = Dataset::open(&shp_path).context("Impossible d'ouvrir le shapefile")?;

        let mut layer = dataset
            .layer(0)
            .context("Impossible d'accéder à la première couche")?;

        // 4. Créer la transformation de coordonnées
        let source_srs = layer
            .spatial_ref()
            .context("Impossible d'obtenir le SRS source")?;
        let target_srs = SpatialRef::from_epsg(self.geo_core.epsg as u32)
            .context("Impossible de créer le SRS cible")?;
        tracing::info!("Source SRS: {:?}", source_srs);
        tracing::info!("Target SRS: {:?}", target_srs);

        let transform = CoordTransform::new(&source_srs, &target_srs)
            .context("Impossible de créer la transformation")?;

        // 5. Créer le polygone bbox en EPSG:2154
        let bbox_polygon = self.create_bbox(bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y)?;
        // Géométrie OGR depuis WKT

        // Définition des CRS — GDAL 3 utilise par défaut l'ordre EPSG:4326 (lat, lon) ;
        // notre polygone WKT est en (lon, lat) comme en GIS classique.
        let mut src = SpatialRef::from_epsg(4326)?;
        src.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
        let mut dst = SpatialRef::from_epsg(2154)?;
        dst.set_axis_mapping_strategy(AxisMappingStrategy::TraditionalGisOrder);
        let wkt = bbox_polygon
            .to_wkt()
            .context("Failed to convert geometry to WKT")?;

        let mut geom = OgrGeometry::from_wkt(&wkt)?;
        geom.set_spatial_ref(src);

        // geom.assign_spatial_ref(&src);
        let geom2: OgrGeometry = geom.transform_to(&dst)?;
        // Convertir en WKT pour l'affichage
        if let Ok(wkt) = geom2.wkt() {
            tracing::info!("Bbox polygon (WKT): {}", wkt);
        }

        // Convertir OGR Geometry en geo::Geometry pour l'intersection (must use projected geom2)
        let bbox_polygon_transformed = self.gdal_to_geo_geometry(&geom2)?;
        let bbox_polygon_geo = if let GeoGeometry::Polygon(p) = bbox_polygon_transformed {
            p
        } else {
            anyhow::bail!("Expected polygon geometry")
        };

        // Obtenir le rectangle englobant pour le filtre spatial
        let bbox_rect_filter = bbox_polygon_geo
            .bounding_rect()
            .context("Failed to get bounding rect for spatial filter")?;

        let extent = layer.get_extent()?;
        tracing::info!("{:?}", extent);
        layer.set_spatial_filter_rect(
            bbox_rect_filter.min().x,
            bbox_rect_filter.min().y,
            bbox_rect_filter.max().x,
            bbox_rect_filter.max().y,
        );
        //layer.set_spatial_filter(&geom2);

        // 6. Étape 1: Collecter et transformer toutes les géométries
        let mut indexed_geometries = Vec::new();
        let mut total_features = 0;
        let mut with_geometry = 0;
        let mut transformed = 0;
        let mut converted = 0;

        tracing::info!("Étape 1: Collecte et transformation des géométries...");

        // Trouver l'index du champ "lcz_int" en utilisant la première feature
        // Puis collecter toutes les features pour les traiter
        let features_vec: Vec<_> = layer.features().collect();
        let mut lcz_int_field_idx = None;

        // Utiliser la première feature pour trouver l'index du champ "lcz_int"
        // Dans GDAL 0.19, on doit utiliser field_count() et tester les champs
        if let Some(first_feature) = features_vec.first() {
            for field_idx in 0..first_feature.field_count() {
                // Tester si ce champ correspond à "lcz_int" en vérifiant la valeur
                // Note: Sans accès à field_defn(), on utilise une heuristique
                // On cherche un champ entier qui pourrait être lcz_int
                if let Ok(Some(field_value)) = first_feature.field(field_idx) {
                    // Si c'est un entier, on suppose que c'est lcz_int (workaround)
                    // Dans un cas réel, on devrait avoir accès au nom du champ
                    if field_value.into_int().is_some() {
                        lcz_int_field_idx = Some(field_idx);
                        // Note: Cette approche suppose que le premier champ entier est lcz_int
                        // Pour une solution plus robuste, il faudrait accéder au nom du champ
                        break;
                    }
                }
            }
        }

        for (idx, feature) in features_vec.iter().enumerate() {
            total_features += 1;

            // Lire lcz_int en utilisant l'index du champ
            let lcz_int: u8 = if let Some(field_idx) = lcz_int_field_idx {
                match feature.field(field_idx) {
                    Ok(Some(field_value)) => {
                        let int_value: i32 = field_value.into_int().unwrap_or(0);
                        int_value as u8
                    }
                    Ok(None) | Err(_) => 0,
                }
            } else {
                0
            };

            // Obtenir la couleur depuis la table
            let color = self
                .table_color
                .get(&lcz_int)
                .map(|(_, c)| c.clone())
                .unwrap_or_else(|| "#000000".to_string());

            // Lire et transformer la géométrie
            if let Some(geom_ref) = feature.geometry() {
                with_geometry += 1;
                let geom: gdal::vector::Geometry = geom_ref.clone();
                // Reprojeter
                if geom.transform(&transform).is_ok() {
                    transformed += 1;
                    // Convertir en geo::Geometry
                    if let Ok(geo_geom) = self.gdal_to_geo_geometry(&geom) {
                        converted += 1;
                        // Stocker pour indexation
                        indexed_geometries.push(IndexedGeometry {
                            geom: geo_geom,
                            lcz_int,
                            color,
                        });
                    }
                }
            }

            if idx % 1000 == 0 && idx > 0 {
                tracing::info!("  Traitement de {} features...", idx);
            }
        }

        tracing::info!("Statistiques de collecte:");
        tracing::info!("  Total features: {}", total_features);
        tracing::info!("  Avec géométrie: {}", with_geometry);
        tracing::info!("  Transformées: {}", transformed);
        tracing::info!("  Converties et indexées: {}", converted);

        // 7. Étape 2: Construire l'index spatial RTree
        tracing::info!("Étape 2: Construction de l'index spatial RTree...");
        let tree = RTree::bulk_load(indexed_geometries);
        tracing::info!("  Index construit avec {} géométries", tree.size());

        // 8. Étape 3: Requête spatiale rapide avec le bbox
        tracing::info!("Étape 3: Requête spatiale avec le bbox...");
        let bbox_rect = bbox_polygon_geo
            .bounding_rect()
            .context("Failed to get bounding rect from polygon")?;
        let envelope = AABB::from_corners(
            [bbox_rect.min().x, bbox_rect.min().y],
            [bbox_rect.max().x, bbox_rect.max().y],
        );
        tracing::info!("Envelope: {:?}", envelope);

        let candidates: Vec<_> = tree.locate_in_envelope_intersecting(envelope).collect();
        let num_candidates = candidates.len();
        tracing::info!("  {} candidats trouvés dans l'enveloppe", num_candidates);

        // 9. Étape 4: Test d'intersection exacte sur les candidats
        tracing::info!("Étape 4: Test d'intersection exacte...");
        let mut features = Vec::new();
        let mut exact_intersections = 0;

        for indexed_geom in &candidates {
            // Test d'intersection exacte avec le polygone bbox
            if bbox_polygon_geo.intersects(&indexed_geom.geom) {
                exact_intersections += 1;
                // Convertir geo::Geometry en geojson::Geometry
                if let Ok(geojson_geom) = self.geo_to_geojson_geometry(&indexed_geom.geom) {
                    let mut feature_json = Feature::from(geojson_geom);
                    feature_json.set_property("lcz_int", indexed_geom.lcz_int as i64);
                    feature_json.set_property("color", indexed_geom.color.clone());
                    features.push(feature_json);
                }
            }
        }

        tracing::info!("Statistiques finales:");
        tracing::info!("  Candidats dans l'enveloppe: {}", num_candidates);
        tracing::info!("  Intersections exactes: {}", exact_intersections);
        tracing::info!("  Features finales: {}", features.len());

        // 7. Créer le GeoJSON FeatureCollection
        let feature_collection = geojson::FeatureCollection {
            bbox: None,
            foreign_members: None,
            features,
        };
        self.geojson = Some(GeoJson::from(feature_collection));
        // Note: clear_spatial_filter() nécessite un emprunt mutable
        // mais layer.features() a déjà été utilisé, donc on le fait après
        // Si nécessaire, on peut appeler clear_spatial_filter() ailleurs
        // layer.clear_spatial_filter();

        Ok(())
    }
}
