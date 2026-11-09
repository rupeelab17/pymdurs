//! Load buildings from GeoJSON, shapefile, or IGN API.

use anyhow::{Context, Result};
#[cfg(not(feature = "wasm"))]
#[allow(unused_imports)]
use gdal::vector::Feature as GdalFeature;
use geo::Polygon;
use geojson::{Feature as GeoJsonFeature, GeoJson, Geometry};
use std::path::Path;

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::BoundingBox;

use super::types::{Building, BuildingCollection};

impl BuildingCollection {
    /// Load buildings from a Shapefile
    /// NOTE: Temporarily disabled due to GDAL API issues
    /// TODO: Fix GDAL integration
    #[allow(dead_code)]
    pub fn from_shapefile<P: AsRef<Path>>(
        _filepath: P,
        _output_path: Option<String>,
        _default_storey_height: f64,
        _set_crs: Option<i32>,
    ) -> Result<Self> {
        anyhow::bail!("Shapefile loading is temporarily disabled. Use from_geojson() instead.");
        /*
        let mut collection = Self::new(Some(filepath.as_ref().to_string_lossy().to_string()), output_path, default_storey_height, set_crs)?;

        let dataset = Dataset::open(filepath.as_ref())
            .context("Failed to open shapefile")?;

        let layer = dataset.layer(0)
            .context("Failed to get layer from dataset")?;

        // Handle CRS
        if let Some(epsg) = set_crs {
            collection.set_crs(epsg);
        } else if let Some(srs) = layer.spatial_ref() {
            if let Some(epsg_code) = srs.to_epsg() {
                collection.set_crs(epsg_code);
            }
        }

        // Iterate through features
        for (idx, feature) in layer.features().enumerate() {
            let feature = feature?;

            if let Some(building) = Self::feature_to_building(&feature, default_storey_height)? {
                collection.add_building(building);
            }
        }

        Ok(collection)
        */
    }

    /// Load buildings from GeoJSON (file or bytes)
    pub fn from_geojson(
        geojson_data: &[u8],
        output_path: Option<String>,
        default_storey_height: f64,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        let mut collection = Self::new(None, output_path, default_storey_height, set_crs)?;

        // Parse GeoJSON using geojson crate
        let geojson_str =
            std::str::from_utf8(geojson_data).context("GeoJSON data is not valid UTF-8")?;
        let geojson: GeoJson = geojson_str.parse().context("Failed to parse GeoJSON")?;

        // Extract features
        match geojson {
            GeoJson::FeatureCollection(fc) => {
                for feature in fc.features {
                    // Skip features that are not polygons (continue processing)
                    match Self::geojson_feature_to_building(&feature, default_storey_height) {
                        Ok(Some(building)) => {
                            collection.add_building(building);
                        }
                        Ok(None) => {
                            // Not a polygon - skip and continue
                            continue;
                        }
                        Err(e) => {
                            // Log error but continue processing other features
                            eprintln!("Warning: Failed to process feature: {}", e);
                            continue;
                        }
                    }
                }
            }
            GeoJson::Feature(f) => {
                // Skip features that are not polygons
                match Self::geojson_feature_to_building(&f, default_storey_height) {
                    Ok(Some(building)) => {
                        collection.add_building(building);
                    }
                    Ok(None) => {
                        // Not a polygon - skip
                    }
                    Err(e) => {
                        // Log error but don't fail completely
                        eprintln!("Warning: Failed to process feature: {}", e);
                    }
                }
            }
            _ => {
                anyhow::bail!("GeoJSON must be a Feature or FeatureCollection");
            }
        }

        Ok(collection)
    }

    /// Load buildings from IGN API
    pub fn from_ign_api(
        output_path: Option<String>,
        default_storey_height: f64,
        bbox: Option<BoundingBox>,
    ) -> Result<Self> {
        let mut ign_collect = IgnCollect::new()?;

        if let Some(bbox) = bbox {
            ign_collect.bbox = Some(bbox);
        } else {
            anyhow::bail!("Bounding box is required for IGN API requests");
        }

        // Execute IGN API request
        ign_collect.execute_ign("buildings")?;

        // Get GeoJSON content as bytes
        let geojson_bytes = ign_collect
            .content
            .as_ref()
            .context("No content available from IGN API")?;

        Self::from_geojson(geojson_bytes, output_path, default_storey_height, None)
    }

    /// Convert a GDAL feature to a Building
    /// NOTE: Temporarily disabled due to GDAL API issues
    #[allow(dead_code)]
    fn feature_to_building(
        _feature: &GdalFeature,
        _default_storey_height: f64,
    ) -> Result<Option<Building>> {
        anyhow::bail!("GDAL feature conversion is temporarily disabled");
        /*
        // Get geometry
        let geometry = feature.geometry()
            .context("Feature has no geometry")?;

        // Convert GDAL geometry to geo::Polygon
        let polygon = Self::gdal_geometry_to_polygon(&geometry)?;

        let mut building = Building::new(polygon);

        // Extract attributes
        for field_idx in 0..feature.field_count() {
            let field_defn = feature.field_defn(field_idx)
                .context("Failed to get field definition")?;

            let field_name = field_defn.name();
            let field_value = feature.field(field_idx);

            match field_name.to_lowercase().as_str() {
                "hauteur" | "height" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_height(f);
                        }
                    }
                }
                "nombre_d_etages" | "storeys" | "etages" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_nombre_d_etages(f);
                        }
                    }
                }
                "hauteur_2" | "height_2" | "h2" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_hauteur_2(f);
                        }
                    }
                }
                _ => {
                    // Store other fields as metadata
                    if let Some(s) = field_value.as_string() {
                        building.metadata.insert(field_name.to_string(), s);
                    }
                }
            }
        }

        Ok(Some(building))
        */
    }

    /// Convert GeoJSON feature to Building
    /// Returns None if geometry is not a polygon (allows skipping non-polygon features)
    fn geojson_feature_to_building(
        feature: &GeoJsonFeature,
        _default_storey_height: f64,
    ) -> Result<Option<Building>> {
        let geometry = feature
            .geometry
            .as_ref()
            .context("Feature has no geometry")?;

        // Get polygon from geometry (handles Polygon and MultiPolygon)
        // Returns None if not a polygon type - we skip these features
        let polygon = match Self::geojson_geometry_to_polygon(geometry)? {
            Some(poly) => poly,
            None => {
                // Not a polygon - skip this feature
                return Ok(None);
            }
        };

        let mut building = Building::new(polygon);

        // Extract properties
        if let Some(properties) = &feature.properties {
            for (key, value) in properties {
                match key.to_lowercase().as_str() {
                    "hauteur" | "height" => {
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_height(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_height(n as f64);
                            }
                        }
                    }
                    "nombre_d_etages" | "storeys" | "etages" => {
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_nombre_d_etages(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_nombre_d_etages(n as f64);
                            }
                        }
                    }
                    "hauteur_2" | "height_2" | "h2" => {
                        // Python uses "HAUTEUR_2" (uppercase), but we also check lowercase variants
                        // Following Python: elif "HAUTEUR_2" in gdf.columns
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_hauteur_2(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_hauteur_2(n as f64);
                            }
                        }
                    }
                    _ => {
                        if let Some(s) = value.as_str() {
                            building.metadata.insert(key.clone(), s.to_string());
                        } else if let Some(f) = value.as_f64() {
                            building.metadata.insert(key.clone(), f.to_string());
                        } else if let Some(n) = value.as_i64() {
                            building.metadata.insert(key.clone(), n.to_string());
                        }
                    }
                }
            }
        }

        Ok(Some(building))
    }

    /// Convert GDAL geometry to geo::Polygon
    /// NOTE: Temporarily disabled due to GDAL API issues
    #[allow(dead_code)]
    fn gdal_geometry_to_polygon(_geometry: &gdal::vector::Geometry) -> Result<Polygon<f64>> {
        anyhow::bail!("GDAL geometry conversion is temporarily disabled");
        /*
        // Get WKT representation and parse it
        let wkt = geometry.wkt()
            .context("Failed to get WKT from geometry")?;

        // Use geos to parse WKT (more reliable than manual parsing)
        let geos_geom = geos::Geometry::try_from(geometry)
            .context("Failed to convert GDAL geometry to GEOS")?;

        // Convert GEOS to geo
        let geo_geom: geo::Geometry<f64> = geos_geom.try_into()
            .context("Failed to convert GEOS geometry to geo")?;

        match geo_geom {
            geo::Geometry::Polygon(poly) => Ok(poly),
            _ => anyhow::bail!("Geometry is not a polygon"),
        }
        */
    }

    /// Convert GeoJSON geometry to Polygon
    /// Handles Polygon and MultiPolygon (takes first polygon from MultiPolygon)
    /// Returns None if geometry is not a polygon type (allows skipping non-polygon features)
    fn geojson_geometry_to_polygon(geometry: &Geometry) -> Result<Option<Polygon<f64>>> {
        // Convert geojson::Geometry to geo::Geometry
        let geo_geom: geo::Geometry<f64> = geometry
            .try_into()
            .context("Failed to convert GeoJSON geometry to geo::Geometry")?;

        match geo_geom {
            geo::Geometry::Polygon(poly) => Ok(Some(poly)),
            geo::Geometry::MultiPolygon(mp) => {
                // Take the first polygon from MultiPolygon
                if let Some(first_poly) = mp.0.first() {
                    Ok(Some(first_poly.clone()))
                } else {
                    Ok(None) // Empty MultiPolygon
                }
            }
            _ => {
                // Not a polygon type - return None to skip this feature
                Ok(None)
            }
        }
    }
}
