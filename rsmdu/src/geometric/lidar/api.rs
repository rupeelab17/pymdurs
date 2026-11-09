//! Public Lidar API (construct, bbox, run, save).

use anyhow::{Context, Result};
use proj::Proj;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::geo_core::{BoundingBox, GeoCore};

use super::types::{log_timing, DtmFill, Lidar};

impl Lidar {

    pub fn new(
        output_path: Option<String>,
        classification: Option<u8>,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Self> {
        use crate::collect::global_variables::TEMP_PATH;
        let output_path_buf = PathBuf::from(
            output_path
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(TEMP_PATH),
        );
        let mut lidar = Lidar {
            geo_core: GeoCore::default(),
            output_path: output_path_buf,
            classification,
            list_path_laz: None,
            loaded_points: None,
            dtm_fill: DtmFill::default(),
        };
        if let Some((min_x, min_y, max_x, max_y)) = bbox {
            lidar.set_bbox(min_x, min_y, max_x, max_y)?;
        }
        Ok(lidar)
    }

    /// Set bbox (WGS84) and resolve COPC/LAZ URLs via IGN WFS.
    /// Does **not** download point data — that happens on first `run()` / `save_las()`.
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<()> {
        self.geo_core
            .set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));
        self.loaded_points = None;
        let _ = self.get_lidar_points()?;
        Ok(())
    }

    /// Download and cache points for the current bbox if not already loaded.
    pub(super) fn ensure_points_loaded(&mut self) -> Result<()> {
        if let Some(ref pts) = self.loaded_points {
            log_timing("load_total", Duration::ZERO, Some(pts.len()));
            return Ok(());
        }
        let laz_urls = self
            .list_path_laz
            .as_ref()
            .context("No LAZ URLs available. Call set_bbox() first.")?;
        if laz_urls.is_empty() {
            anyhow::bail!("No LAZ files found for the specified bounding box");
        }
        let laz_urls = laz_urls.clone();
        let (min_x, min_y, max_x, max_y) = self.bbox_lambert93()?;
        let t0 = Instant::now();
        let points = self.load_lidar_points_internal(&laz_urls, Some((min_x, min_y, max_x, max_y)))?;
        log_timing("load_total", t0.elapsed(), Some(points.len()));
        self.loaded_points = Some(points);
        Ok(())
    }

    /// Current bbox transformed to EPSG:2154 (Lambert-93).
    pub(super) fn bbox_lambert93(&self) -> Result<(f64, f64, f64, f64)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set")?;
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;
        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;
        Ok((min_x, min_y, max_x, max_y))
    }

    /// Choose how DTM holes are filled before the CHM (see [`DtmFill`]).
    pub fn set_dtm_fill(&mut self, mode: DtmFill) {
        self.dtm_fill = mode;
    }

    pub fn set_classification(&mut self, classification: Option<u8>) {
        self.classification = classification;
    }

    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }

    /// COPC tile URLs for the current bbox (populated by `set_bbox` / ctor with bbox).
    ///
    /// Example: `https://data.geopf.fr/.../LHD_FXX_0399_6580_PTS_LAMB93_IGN69.copc.laz`
    pub fn list_copc_urls(&self) -> Result<Vec<String>> {
        let urls = self.list_path_laz.as_ref().context(
            "No COPC/LAZ URLs loaded. Call set_bbox() first.",
        )?;
        Ok(urls
            .iter()
            .filter(|u| Self::is_copc_url(u))
            .cloned()
            .collect())
    }

    /// All LAZ tile URLs (COPC and non-COPC) for the current bbox.
    pub fn list_laz_urls(&self) -> Result<Vec<String>> {
        self.list_path_laz
            .clone()
            .context("No LAZ URLs loaded. Call set_bbox() first.")
    }

    pub fn run(
        &mut self,
        file_name: Option<String>,
        classification_list: Option<Vec<u8>>,
        resolution: Option<f64>,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        let resolution = resolution.unwrap_or(1.0);
        self.ensure_points_loaded()?;
        let (min_x, min_y, max_x, max_y) = self.bbox_lambert93()?;
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded")?;
        let rasters = self.process_lidar_points(
            points,
            (min_x, min_y, max_x, max_y),
            classification_list,
            resolution,
        )?;
        let output_file = self
            .output_path
            .join(file_name.unwrap_or("lidar_cdsm.tif".to_string()));
        let t_tif = Instant::now();
        let output_path = self.to_tif(&rasters, &output_file, write_out_file)?;
        log_timing("geotiff", t_tif.elapsed(), None);
        Ok(output_path)
    }

    #[cfg(feature = "las")]
    pub fn save_las(&mut self, path: &Path) -> Result<PathBuf> {
        self.ensure_points_loaded()?;
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded. Call set_bbox() then run() or save().")?;
        if points.is_empty() {
            anyhow::bail!("No LiDAR points to export.");
        }
        let (min_x, min_y, min_z, _max_x, _max_y, _max_z) = points.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, min_y, min_z, max_x, max_y, max_z), p| {
                (
                    min_x.min(p.x),
                    min_y.min(p.y),
                    min_z.min(p.z),
                    max_x.max(p.x),
                    max_y.max(p.y),
                    max_z.max(p.z),
                )
            },
        );
        // Format 6 (LAS 1.4 extended): full u8 classification — required for IGN LiDAR HD
        // codes 64/66/67. Format 0 only allows 0–31 and rejects those classes.
        let mut builder = las::Builder::from((1, 4));
        builder.point_format = las::point::Format::new(6).context("Invalid point format")?;
        builder.transforms = las::Vector {
            x: las::Transform {
                scale: 0.01,
                offset: min_x,
            },
            y: las::Transform {
                scale: 0.01,
                offset: min_y,
            },
            z: las::Transform {
                scale: 0.01,
                offset: min_z,
            },
        };
        let header = builder
            .into_header()
            .context("Failed to build LAS header")?;
        let out_path: PathBuf = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.output_path.join(path)
        };
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create output directory for LAS")?;
        }
        let mut writer =
            las::Writer::from_path(&out_path, header).context("Failed to create LAS writer")?;
        for p in points {
            let classification = las::point::Classification::new(p.classification)
                .unwrap_or(las::point::Classification::Unclassified);
            let las_point = las::Point {
                x: p.x,
                y: p.y,
                z: p.z,
                classification,
                return_number: 1,
                number_of_returns: 1,
                gps_time: Some(0.0), // required by point format 6
                ..Default::default()
            };
            writer
                .write_point(las_point)
                .map_err(|e| anyhow::anyhow!("Failed to write LAS point: {}", e))?;
        }
        writer.close().context("Failed to close LAS writer")?;
        Ok(out_path)
    }
}
