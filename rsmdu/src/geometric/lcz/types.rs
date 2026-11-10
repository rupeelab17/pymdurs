//! LCZ (Local Climate Zone) types.

use anyhow::{Context, Result};
use geojson::GeoJson;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::geo_core::{BoundingBox, GeoCore};
use geo::algorithm::bounding_rect::BoundingRect;
use geo::Geometry as GeoGeometry;
use rstar::{RTreeObject, AABB};

/// Structure pour indexer les géométries avec rstar
pub(crate) struct IndexedGeometry {
    pub(crate) geom: GeoGeometry<f64>,
    pub(crate) lcz_int: u8,
    pub(crate) color: String,
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
    pub(crate) output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the LCZ area
    pub(crate) bbox: Option<BoundingBox>,
    /// Optional shapefile path (reserved for future use)
    #[allow(dead_code)]
    pub(crate) filepath_shp: Option<String>,
    /// LCZ color table mapping LCZ codes to names and colors
    pub table_color: HashMap<u8, (String, String)>,
    /// Parsed GeoJSON content (after processing)
    pub(crate) geojson: Option<GeoJson>,
}

impl Lcz {
    /// Create a new Lcz instance
    /// Following Python: def __init__(self, filepath_shp=None, output_path=None, set_crs=None)
    pub fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

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

        tracing::info!(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcz_new_default_crs() {
        let l = Lcz::new(None, None, None).unwrap();
        assert_eq!(l.geo_core.get_epsg(), 2154);
        assert!(!l.table_color.is_empty());
        assert!(l.table_color.contains_key(&1));
        assert!(!l.get_output_path().as_os_str().is_empty());
    }

    #[test]
    fn lcz_set_bbox() {
        let mut l = Lcz::new(None, None, Some(4326)).unwrap();
        l.set_bbox(-1.15, 46.18, -1.14, 46.19);
        assert!(l.bbox.is_some());
    }
}
