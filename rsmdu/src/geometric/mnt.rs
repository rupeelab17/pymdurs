use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// MNT (Modèle Numérique de Terrain) issu du LiDAR HD IGN.
pub struct Mnt {
    ign_collect: IgnCollect,
    output_path: PathBuf,
    path_save_tiff: PathBuf,
    path_temp_tiff: PathBuf,
    pub geo_core: GeoCore,
    bbox: Option<BoundingBox>,
}

impl Mnt {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::TEMP_PATH;

        let output_path_buf = PathBuf::from(
            output_path
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(TEMP_PATH),
        );

        let path_save_tiff = output_path_buf.join("mnt_lidar_hd.tif");
        let path_temp_tiff = PathBuf::from(TEMP_PATH).join("mnt.tiff");

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

        Ok(Mnt {
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            path_save_tiff,
            path_temp_tiff,
            geo_core: GeoCore::default(),
            bbox: None,
        })
    }

    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.ign_collect.set_bbox(min_x, min_y, max_x, max_y);
    }

    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core = GeoCore::new(epsg);
        self.ign_collect.geo_core.set_epsg(epsg);
    }

    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    pub fn run_internal(&mut self) -> Result<()> {
        self.ign_collect.execute_ign("mnt")?;

        if !self.path_temp_tiff.exists() {
            anyhow::bail!(
                "MNT file not found at {:?}. Make sure execute_ign('mnt') was called successfully.",
                self.path_temp_tiff
            );
        }

        self.copy_to_output()?;
        Ok(())
    }

    fn copy_to_output(&self) -> Result<()> {
        if let Some(parent) = self.path_save_tiff.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        std::fs::copy(&self.path_temp_tiff, &self.path_save_tiff).context(format!(
            "Failed to copy MNT from {:?} to {:?}",
            self.path_temp_tiff, self.path_save_tiff
        ))?;

        println!("MNT saved to: {:?}", self.path_save_tiff);
        Ok(())
    }

    pub fn content(&self) -> Option<&Vec<u8>> {
        self.ign_collect.content.as_ref()
    }

    pub fn get_path_save_tiff(&self) -> &Path {
        &self.path_save_tiff
    }

    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mnt_new() {
        let mnt = Mnt::new(None).unwrap();
        assert!(mnt
            .path_save_tiff
            .to_string_lossy()
            .contains("mnt_lidar_hd.tif"));
    }

    #[test]
    fn test_mnt_set_bbox() {
        let mut mnt = Mnt::new(None).unwrap();
        mnt.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);
        assert!(mnt.bbox.is_some());
    }

    #[test]
    fn test_mnt_set_crs() {
        let mut mnt = Mnt::new(None).unwrap();
        mnt.set_crs(2154);
        assert_eq!(mnt.geo_core.get_epsg(), 2154);
    }
}
