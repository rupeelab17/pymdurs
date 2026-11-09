use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Cosia structure
/// Following Python implementation from pymdu.geometric.Cosia
/// Provides methods to collect and process Cosia (landcover) data from IGN API
pub struct Cosia {
    /// IgnCollect instance for API requests
    ign_collect: IgnCollect,
    /// Output path for processed data
    output_path: PathBuf,
    /// Path to save the final Cosia TIFF file
    path_save_tiff: PathBuf,
    /// Path to temporary TIFF file from IGN API
    path_temp_tiff: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the Cosia area
    bbox: Option<BoundingBox>,
    /// Optional template raster path (for future use)
    #[allow(dead_code)]
    template_raster_path: Option<PathBuf>,
}

impl Cosia {
    /// Create a new Cosia instance
    /// Following Python: def __init__(self, output_path: str | None = None, template_raster_path: str | None = None)
    pub fn new(output_path: Option<String>, template_raster_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        let path_save_tiff = output_path_buf.join("cosia.tif");
        let path_temp_tiff = get_temp_path().join("cosia.tiff");

        // Remove existing files if they exist (following Python)
        // Python: if os.path.exists(self.path_temp_tiff): os.remove(self.path_temp_tiff)
        if path_temp_tiff.exists() {
            std::fs::remove_file(&path_temp_tiff).context(format!(
                "Failed to remove existing file: {:?}",
                path_temp_tiff
            ))?;
        }
        if path_save_tiff.exists() {
            std::fs::remove_file(&path_save_tiff).context(format!(
                "Failed to remove existing file: {:?}",
                path_save_tiff
            ))?;
        }

        let template_raster = template_raster_path.map(PathBuf::from);

        Ok(Cosia {
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            path_save_tiff,
            path_temp_tiff,
            geo_core: GeoCore::default(), // Default to EPSG:2154 (Lambert-93)
            bbox: None,
            template_raster_path: template_raster,
        })
    }

    /// Set bounding box
    /// Following Python: self.bbox = [...]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.ign_collect.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Set CRS
    /// Following Python: self._epsg (defaults to 2154)
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core = GeoCore::new(epsg);
        self.ign_collect.geo_core.set_epsg(epsg);
    }

    /// Run Cosia processing: download from IGN API
    /// Following Python: def run_ign(self) -> self
    /// Downloads Cosia raster from IGN API and saves it
    pub fn run_ign(mut self) -> Result<Self> {
        self.run_ign_internal()?;
        Ok(self)
    }

    /// Internal run_ign method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_ign_internal(&mut self) -> Result<()> {
        // Python: self.content = self.execute_ign(key="cosia").content
        self.ign_collect.execute_ign("cosia")?;

        // The Cosia TIFF should have been saved to path_temp_tiff by execute_ign
        // Python: dataarray = rxr.open_rasterio(self.path_temp_tiff)
        if !self.path_temp_tiff.exists() {
            anyhow::bail!(
                "Cosia file not found at {:?}. Make sure execute_ign('cosia') was called successfully.",
                self.path_temp_tiff
            );
        }

        // Copy temp file to output (matching Python behavior)
        // Python: dataarray.rio.to_raster(self.path_save_tiff, ...)
        self.copy_to_output()?;

        Ok(())
    }

    /// Copy temporary file to output location
    /// Following Python: dataarray.rio.to_raster(...)
    fn copy_to_output(&self) -> Result<()> {
        // Create output directory if it doesn't exist
        if let Some(parent) = self.path_save_tiff.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        // Copy temp file to output
        std::fs::copy(&self.path_temp_tiff, &self.path_save_tiff).context(format!(
            "Failed to copy Cosia from {:?} to {:?}",
            self.path_temp_tiff, self.path_save_tiff
        ))?;

        tracing::info!("Cosia saved to: {:?}", self.path_save_tiff);

        Ok(())
    }

    /// Get the content from IGN API
    /// Following Python: def content(self): return self.content
    pub fn content(&self) -> Option<&Vec<u8>> {
        self.ign_collect.content.as_ref()
    }

    /// Get path to saved TIFF file
    pub fn get_path_save_tiff(&self) -> &Path {
        &self.path_save_tiff
    }

    /// Get output path
    /// Following Python: self.output_path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosia_new() {
        let cosia = Cosia::new(None, None).unwrap();
        assert!(cosia.path_save_tiff.to_string_lossy().contains("cosia.tif"));
    }

    #[test]
    fn test_cosia_set_bbox() {
        let mut cosia = Cosia::new(None, None).unwrap();
        cosia.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);
        assert!(cosia.bbox.is_some());
    }

    #[test]
    fn test_cosia_set_crs() {
        let mut cosia = Cosia::new(None, None).unwrap();
        cosia.set_crs(2154);
        assert_eq!(cosia.geo_core.get_epsg(), 2154);
    }
}
