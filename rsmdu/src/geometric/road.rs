use anyhow::{Context, Result};
use geojson::GeoJson;
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Road structure
/// Following Python implementation from pymdu.geometric.Road
/// Provides methods to collect and process Road (route) data from IGN API
pub struct Road {
    /// IgnCollect instance for API requests
    ign_collect: IgnCollect,
    /// Output path for processed data
    output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the road area
    bbox: Option<BoundingBox>,
    /// Parsed GeoJSON content
    geojson: Option<GeoJson>,
}

impl Road {
    /// Create a new Road instance
    /// Following Python: def __init__(self, output_path: str | None = None)
    pub fn new(output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        Ok(Road {
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            geo_core: GeoCore::default(), // Default to EPSG:2154 (Lambert-93)
            bbox: None,
            geojson: None,
        })
    }

    /// Set bounding box
    /// Following Python: road.bbox = [min_x, min_y, max_x, max_y]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.ign_collect.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
    }

    /// Set CRS
    /// Following Python: road._epsg = epsg
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core.set_epsg(epsg);
        self.ign_collect.geo_core.set_epsg(epsg);
    }

    /// Run road processing: download from IGN API, parse GeoJSON
    /// Following Python: def run(self) -> self
    pub fn run(mut self) -> Result<Self> {
        // Execute IGN API request for road
        // Python: self.execute_ign(key="road")
        self.ign_collect
            .execute_ign("road")
            .context("Failed to execute IGN request for road")?;

        // Get content from IgnCollect
        let content = self
            .ign_collect
            .content
            .as_ref()
            .context("No content received from IGN API")?;

        // Parse GeoJSON following Python: gpd.read_file(file, driver="GeoJSON")
        // Python: file = self.content if isinstance(self.content, io.BytesIO) else io.BytesIO(self.content)
        //         gdf = gpd.read_file(file, driver="GeoJSON")
        let geojson_str = String::from_utf8_lossy(content);
        let geojson: GeoJson = geojson_str
            .parse()
            .context("Failed to parse GeoJSON from IGN API response")?;

        // Store the parsed GeoJSON
        // Note: Reprojection to target CRS (Python: gdf = gdf.to_crs(self._epsg))
        // would require converting GeoJSON to GDAL Dataset, reprojecting, and converting back
        // This is complex and would require additional dependencies
        // For now, we store the GeoJSON as-is
        // TODO: Implement reprojection using GDAL or proj crate
        self.geojson = Some(geojson);

        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self) -> Result<()> {
        // Execute IGN API request for road
        self.ign_collect
            .execute_ign("road")
            .context("Failed to execute IGN request for road")?;

        // Get content from IgnCollect
        let content = self
            .ign_collect
            .content
            .as_ref()
            .context("No content received from IGN API")?;

        // Parse GeoJSON
        let geojson_str = String::from_utf8_lossy(content);
        let geojson: GeoJson = geojson_str
            .parse()
            .context("Failed to parse GeoJSON from IGN API response")?;

        self.geojson = Some(geojson);

        Ok(())
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    pub fn get_geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    /// Save to GeoJSON file
    /// Following Python: def to_geojson(self, name: str = "routes")
    /// Note: GeoJSON export requires GDAL and is complex
    /// For now, we save as GeoJSON - full GeoJSON export would require GDAL layer operations
    /// TODO: Implement full GeoJSON export using GDAL
    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        // Python: self.gdf.to_file(f"{os.path.join(self.output_path, name)}.GeoJSON", driver="GeoJSON")
        // For now, save as GeoJSON as a workaround
        // Full GeoJSON export would require:
        // 1. Converting GeoJSON to GDAL Dataset
        // 2. Reprojecting to target CRS if needed
        // 3. Creating GeoJSON file with GDAL driver
        // 4. Copying layers and features

        let geojson = self
            .geojson
            .as_ref()
            .context("No GeoJSON data available. Call run() first.")?;

        let name = name.unwrap_or("road");

        // Save as GeoJSON for now (GeoJSON export is complex with GDAL Rust bindings)
        let output_file = self.output_path.join(format!("{}.geojson", name));
        let geojson_str = geojson.to_string();
        std::fs::write(&output_file, geojson_str)
            .context(format!("Failed to write GeoJSON file: {:?}", output_file))?;

        tracing::info!(
            "Road saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
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
    fn road_new() {
        let r = Road::new(None).unwrap();
        assert!(!r.get_output_path().as_os_str().is_empty());
        assert_eq!(r.geo_core.get_epsg(), 2154);
    }

    #[test]
    fn road_set_bbox_crs() {
        let mut r = Road::new(None).unwrap();
        r.set_bbox(-1.15, 46.18, -1.14, 46.19);
        r.set_crs(2154);
        assert!(r.bbox.is_some());
        assert_eq!(r.geo_core.get_epsg(), 2154);
    }
}
