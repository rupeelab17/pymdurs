//! MNT / MNS / MNH — rasters d'élévation LiDAR HD IGN (WMS-R).

use anyhow::{Context, Result};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Identifiant du produit IGN (`mnt` / `mns` / `mnh`).
pub trait RasterKind {
    const KEY: &'static str;
    const LABEL: &'static str;
}

pub struct MntKind;
pub struct MnsKind;
pub struct MnhKind;

impl RasterKind for MntKind {
    const KEY: &'static str = "mnt";
    const LABEL: &'static str = "MNT";
}
impl RasterKind for MnsKind {
    const KEY: &'static str = "mns";
    const LABEL: &'static str = "MNS";
}
impl RasterKind for MnhKind {
    const KEY: &'static str = "mnh";
    const LABEL: &'static str = "MNH";
}

/// Raster d'élévation LiDAR HD IGN.
pub struct LidarRaster<K: RasterKind> {
    ign_collect: IgnCollect,
    output_path: PathBuf,
    path_save_tiff: PathBuf,
    path_temp_tiff: PathBuf,
    pub geo_core: GeoCore,
    bbox: Option<BoundingBox>,
    _kind: PhantomData<K>,
}

pub type Mnt = LidarRaster<MntKind>;
pub type Mns = LidarRaster<MnsKind>;
pub type Mnh = LidarRaster<MnhKind>;

impl<K: RasterKind> LidarRaster<K> {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);
        let path_save_tiff = output_path_buf.join(format!("{}_lidar_hd.tif", K::KEY));
        let path_temp_tiff = get_temp_path().join(format!("{}.tiff", K::KEY));

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

        Ok(Self {
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            path_save_tiff,
            path_temp_tiff,
            geo_core: GeoCore::default(),
            bbox: None,
            _kind: PhantomData,
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
        self.ign_collect.execute_ign(K::KEY)?;

        if !self.path_temp_tiff.exists() {
            anyhow::bail!(
                "{} file not found at {:?}. Make sure execute_ign('{}') was called successfully.",
                K::LABEL,
                self.path_temp_tiff,
                K::KEY
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
            "Failed to copy {} from {:?} to {:?}",
            K::LABEL,
            self.path_temp_tiff,
            self.path_save_tiff
        ))?;

        tracing::info!("{} saved to: {:?}", K::LABEL, self.path_save_tiff);
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
    fn test_new_paths() {
        assert!(Mnt::new(None)
            .unwrap()
            .path_save_tiff
            .to_string_lossy()
            .contains("mnt_lidar_hd.tif"));
        assert!(Mns::new(None)
            .unwrap()
            .path_save_tiff
            .to_string_lossy()
            .contains("mns_lidar_hd.tif"));
        assert!(Mnh::new(None)
            .unwrap()
            .path_save_tiff
            .to_string_lossy()
            .contains("mnh_lidar_hd.tif"));
    }

    #[test]
    fn test_set_bbox() {
        let mut mnt = Mnt::new(None).unwrap();
        mnt.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);
        assert!(mnt.bbox.is_some());
    }

    #[test]
    fn test_set_crs() {
        let mut mnt = Mnt::new(None).unwrap();
        mnt.set_crs(2154);
        assert_eq!(mnt.geo_core.get_epsg(), 2154);
    }
}
