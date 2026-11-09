//! LCZ ZIP I/O and geometry helpers.

use anyhow::{Context, Result};
use geojson::Geometry;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::ZipArchive;

use geo::algorithm::intersects::Intersects;
use geo::{Geometry as GeoGeometry, Polygon};
use geos::{CoordSeq, CoordType, GResult, Geometry as GeosGeometry};

use super::types::Lcz;

impl Lcz {
    /// Download and extract ZIP file
    #[allow(dead_code)]
    pub(crate) fn download_and_extract_zip(&self, url: &str) -> Result<TempDir> {
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
            let Some(outpath) = file.enclosed_name().map(|n| temp_dir.path().join(n)) else {
                continue;
            };

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
    pub(crate) fn find_shapefile(&self, temp_dir: &TempDir) -> Result<PathBuf> {
        // Recherche récursive dans le dossier temporaire
        self.find_shapefile_recursive(temp_dir.path())
    }

    #[allow(dead_code)]
    pub(crate) fn find_shapefile_recursive(&self, dir: &Path) -> Result<PathBuf> {
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

    pub(crate) fn create_bbox(&self, xmin: f64, ymin: f64, xmax: f64, ymax: f64) -> GResult<GeosGeometry> {
        // 5 points (fermeture du polygone), 2 dimensions (x, y)
        let mut coords = CoordSeq::new(5, CoordType::XY)?;

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
    pub(crate) fn gdal_to_geo_geometry(&self, geom: &gdal::vector::Geometry) -> Result<GeoGeometry<f64>> {
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
    pub(crate) fn geo_to_geojson_geometry(&self, geom: &GeoGeometry<f64>) -> Result<Geometry> {
        // Use geojson's From trait
        let geojson_geom: Geometry = geom
            .try_into()
            .context("Failed to convert geo geometry to GeoJSON geometry")?;
        Ok(geojson_geom)
    }

    /// Check if geometry intersects with bbox
    #[allow(dead_code)]
    pub(crate) fn geometry_intersects_bbox(&self, geom: &GeoGeometry<f64>, bbox: &Polygon<f64>) -> bool {
        bbox.intersects(geom)
    }
}
