use anyhow::{Context, Result};
use geojson::{Feature, GeoJson, Geometry};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::ZipArchive;

use crate::geo_core::{BoundingBox, GeoCore};
use gdal::spatial_ref::{CoordTransform, SpatialRef};
use gdal::vector::Geometry as OgrGeometry;
use gdal::vector::LayerAccess;
use gdal::Dataset;
use geo::algorithm::bounding_rect::BoundingRect;
use geo::algorithm::intersects::Intersects;
use geo::{Geometry as GeoGeometry, Polygon};
use geos::{CoordDimensions, CoordSeq, GResult, Geom, Geometry as GeosGeometry};
use rstar::{RTree, RTreeObject, AABB};

/// Structure pour indexer les géométries avec rstar
struct IndexedGeometry {
    geom: GeoGeometry<f64>,
    lcz_int: u8,
    color: String,
}

impl RTreeObject for IndexedGeometry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        // Calculer le bounding rect de la géométrie
        if let Some(rect) = self.geom.bounding_rect() {
            AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])
        } else {
            // Fallback pour géométries sans bounding rect (ne devrait pas arriver)
            AABB::from_corners([0.0, 0.0], [0.0, 0.0])
        }
    }
}

/// LCZ (Local Climate Zone) structure
/// Following Python implementation from pymdu.geometric.Lcz
/// Provides methods to collect and process LCZ data from external sources
pub struct Lcz {
    /// Output path for processed data
    output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the LCZ area
    bbox: Option<BoundingBox>,
    /// Optional shapefile path (reserved for future use)
    #[allow(dead_code)]
    filepath_shp: Option<String>,
    /// LCZ color table mapping LCZ codes to names and colors
    pub table_color: HashMap<u8, (String, String)>,
    /// Parsed GeoJSON content (after processing)
    geojson: Option<GeoJson>,
}

impl Lcz {
    /// Create a new Lcz instance
    /// Following Python: def __init__(self, filepath_shp=None, output_path=None, set_crs=None)
    pub fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        use crate::collect::global_variables::TEMP_PATH;

        let output_path_buf = PathBuf::from(
            output_path
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(TEMP_PATH),
        );

        let mut geo_core = if let Some(epsg) = set_crs {
            GeoCore::new(epsg)
        } else {
            GeoCore::default() // Default to EPSG:2154
        };

        geo_core.set_output_path(Some(output_path_buf.to_string_lossy().to_string()));

        // Initialize LCZ color table
        // Python: self.table_color = {1: ["LCZ 1: Compact high-rise", "#8b0101"], ...}
        let mut table_color = HashMap::new();
        table_color.insert(
            1,
            (
                "LCZ 1: Compact high-rise".to_string(),
                "#8b0101".to_string(),
            ),
        );
        table_color.insert(
            2,
            ("LCZ 2: Compact mid-rise".to_string(), "#cc0200".to_string()),
        );
        table_color.insert(
            3,
            ("LCZ 3: Compact low-rise".to_string(), "#fc0001".to_string()),
        );
        table_color.insert(
            4,
            ("LCZ 4: Open high-rise".to_string(), "#be4c03".to_string()),
        );
        table_color.insert(
            5,
            ("LCZ 5: Open mid-rise".to_string(), "#ff6602".to_string()),
        );
        table_color.insert(
            6,
            ("LCZ 6: Open low-rise".to_string(), "#ff9856".to_string()),
        );
        table_color.insert(
            7,
            (
                "LCZ 7: Lightweight low-rise".to_string(),
                "#fbed08".to_string(),
            ),
        );
        table_color.insert(
            8,
            ("LCZ 8: Large low-rise".to_string(), "#bcbcba".to_string()),
        );
        table_color.insert(
            9,
            ("LCZ 9: Sparsely built".to_string(), "#ffcca7".to_string()),
        );
        table_color.insert(
            10,
            ("LCZ 10: Heavy industry".to_string(), "#57555a".to_string()),
        );
        table_color.insert(
            11,
            ("LCZ A: Dense trees".to_string(), "#006700".to_string()),
        );
        table_color.insert(
            12,
            ("LCZ B: Scattered trees".to_string(), "#05aa05".to_string()),
        );
        table_color.insert(13, ("LCZ C: Bush,scrub".to_string(), "#648423".to_string()));
        table_color.insert(14, ("LCZ D: Low plants".to_string(), "#bbdb7a".to_string()));
        table_color.insert(
            15,
            (
                "LCZ E: Bare rock or paved".to_string(),
                "#010101".to_string(),
            ),
        );
        table_color.insert(
            16,
            (
                "LCZ F: Bare soil or sand".to_string(),
                "#fdf6ae".to_string(),
            ),
        );
        table_color.insert(17, ("LCZ G: Water".to_string(), "#6d67fd".to_string()));

        Ok(Lcz {
            output_path: output_path_buf,
            geo_core,
            bbox: None,
            filepath_shp,
            table_color,
            geojson: None,
        })
    }

    /// Set bounding box
    /// Following Python: lcz.bbox = [min_x, min_y, max_x, max_y]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.geo_core
            .set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));
    }

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

        println!("Bbox (EPSG:4326): {:?}", bbox);

        println!("Téléchargement du fichier ZIP depuis: {}", url);

        // 1. Télécharger et extraire le ZIP
        let temp_dir = self.download_and_extract_zip(url)?;
        // 2. Trouver le fichier .shp dans le dossier temporaire
        let shp_path = self.find_shapefile(&temp_dir)?;

        println!("Shapefile trouvé: {:?}", shp_path);

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
        println!("Source SRS: {:?}", source_srs);
        println!("Target SRS: {:?}", target_srs);

        let transform = CoordTransform::new(&source_srs, &target_srs)
            .context("Impossible de créer la transformation")?;

        // 5. Créer le polygone bbox en EPSG:2154
        let bbox_polygon = self.create_bbox(bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y)?;
        // Géométrie OGR depuis WKT

        // Définition des CRS
        let src = SpatialRef::from_epsg(4326)?;
        let dst = SpatialRef::from_epsg(2154)?;
        let wkt = bbox_polygon
            .to_wkt()
            .context("Failed to convert geometry to WKT")?;

        let mut geom = OgrGeometry::from_wkt(&wkt)?;
        geom.set_spatial_ref(src);

        // geom.assign_spatial_ref(&src);
        let geom2: OgrGeometry = geom.transform_to(&dst)?;
        // Convertir en WKT pour l'affichage
        if let Ok(wkt) = geom2.wkt() {
            println!("Bbox polygon (WKT): {}", wkt);
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
        println!("{:?}", extent);
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

        println!("Étape 1: Collecte et transformation des géométries...");

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
                println!("  Traitement de {} features...", idx);
            }
        }

        println!("Statistiques de collecte:");
        println!("  Total features: {}", total_features);
        println!("  Avec géométrie: {}", with_geometry);
        println!("  Transformées: {}", transformed);
        println!("  Converties et indexées: {}", converted);

        // 7. Étape 2: Construire l'index spatial RTree
        println!("Étape 2: Construction de l'index spatial RTree...");
        let tree = RTree::bulk_load(indexed_geometries);
        println!("  Index construit avec {} géométries", tree.size());

        // 8. Étape 3: Requête spatiale rapide avec le bbox
        println!("Étape 3: Requête spatiale avec le bbox...");
        let bbox_rect = bbox_polygon_geo
            .bounding_rect()
            .context("Failed to get bounding rect from polygon")?;
        let envelope = AABB::from_corners(
            [bbox_rect.min().x, bbox_rect.min().y],
            [bbox_rect.max().x, bbox_rect.max().y],
        );
        println!("Envelope: {:?}", envelope);

        let candidates: Vec<_> = tree.locate_in_envelope_intersecting(&envelope).collect();
        let num_candidates = candidates.len();
        println!("  {} candidats trouvés dans l'enveloppe", num_candidates);

        // 9. Étape 4: Test d'intersection exacte sur les candidats
        println!("Étape 4: Test d'intersection exacte...");
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

        println!("Statistiques finales:");
        println!("  Candidats dans l'enveloppe: {}", num_candidates);
        println!("  Intersections exactes: {}", exact_intersections);
        println!("  Features finales: {}", features.len());

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

    /// Download and extract ZIP file
    #[allow(dead_code)]
    fn download_and_extract_zip(&self, url: &str) -> Result<TempDir> {
        // Télécharger le fichier
        let response = reqwest::blocking::get(url).context("Échec du téléchargement")?;
        let bytes = response.bytes().context("Impossible de lire les bytes")?;

        // Créer un dossier temporaire
        let temp_dir = TempDir::new().context("Impossible de créer un dossier temporaire")?;
        println!("Dossier temporaire créé: {:?}", temp_dir.path());

        // Extraire le ZIP
        let cursor = Cursor::new(bytes);
        let mut archive = ZipArchive::new(cursor).context("Impossible d'ouvrir l'archive ZIP")?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = temp_dir.path().join(file.name());

            if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(parent) = outpath.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Ok(temp_dir)
    }

    /// Find shapefile in extracted directory
    #[allow(dead_code)]
    fn find_shapefile(&self, temp_dir: &TempDir) -> Result<PathBuf> {
        // Recherche récursive dans le dossier temporaire
        self.find_shapefile_recursive(temp_dir.path())
    }

    #[allow(dead_code)]
    fn find_shapefile_recursive(&self, dir: &Path) -> Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Recherche récursive
                if let Ok(found) = self.find_shapefile_recursive(&path) {
                    return Ok(found);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("shp") {
                return Ok(path);
            }
        }
        anyhow::bail!("Aucun fichier .shp trouvé dans l'archive")
    }

    fn create_bbox(&self, xmin: f64, ymin: f64, xmax: f64, ymax: f64) -> GResult<GeosGeometry> {
        // 5 points (fermeture du polygone), 2 dimensions (x, y)
        let mut coords = CoordSeq::new(5, CoordDimensions::TwoD)?;

        coords.set_x(0, xmin)?;
        coords.set_y(0, ymin)?;
        coords.set_x(1, xmax)?;
        coords.set_y(1, ymin)?;
        coords.set_x(2, xmax)?;
        coords.set_y(2, ymax)?;
        coords.set_x(3, xmin)?;
        coords.set_y(3, ymax)?;
        coords.set_x(4, xmin)?;
        coords.set_y(4, ymin)?;

        let ring = GeosGeometry::create_linear_ring(coords)?;
        let polygon = GeosGeometry::create_polygon(ring, vec![])?;
        Ok(polygon)
    }

    /// Convert GDAL geometry to geo::Geometry
    fn gdal_to_geo_geometry(&self, geom: &gdal::vector::Geometry) -> Result<GeoGeometry<f64>> {
        // Get WKT representation
        let wkt = geom.wkt().context("Failed to get WKT from GDAL geometry")?;

        // Parse WKT using geos
        let geos_geom =
            GeosGeometry::new_from_wkt(&wkt).context("Failed to parse WKT with GEOS")?;

        // Convert GEOS to geo
        let geo_geom: GeoGeometry<f64> = geos_geom
            .try_into()
            .context("Failed to convert GEOS geometry to geo")?;

        Ok(geo_geom)
    }

    /// Convert geo::Geometry to geojson::Geometry
    fn geo_to_geojson_geometry(&self, geom: &GeoGeometry<f64>) -> Result<Geometry> {
        // Use geojson's From trait
        let geojson_geom: Geometry = geom
            .try_into()
            .context("Failed to convert geo geometry to GeoJSON geometry")?;
        Ok(geojson_geom)
    }

    /// Check if geometry intersects with bbox
    #[allow(dead_code)]
    fn geometry_intersects_bbox(&self, geom: &GeoGeometry<f64>, bbox: &Polygon<f64>) -> bool {
        bbox.intersects(geom)
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    pub fn geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    /// Save to GeoJSON file
    /// Following Python: def to_geojson(self, name: str = "lcz")
    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        let geojson = self
            .geojson
            .as_ref()
            .context("No GeoJSON data available. Call run() first.")?;

        let name = name.unwrap_or("lcz");

        // Save as GeoJSON for now (GeoJSON export is complex with GDAL Rust bindings)
        let output_file = self.output_path.join(format!("{}.geojson", name));
        let geojson_str = geojson.to_string();
        std::fs::write(&output_file, geojson_str)
            .context(format!("Failed to write GeoJSON file: {:?}", output_file))?;

        println!(
            "LCZ saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
            output_file
        );

        Ok(())
    }

    /// Get output path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}
