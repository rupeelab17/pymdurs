//! Public Lidar API (construct, bbox, run, save).

use anyhow::{Context, Result};
use proj::Proj;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::geo_core::{BoundingBox, GeoCore};

use super::types::{log_timing, DtmFill, Lidar, TimingSpan};

impl Lidar {
    pub fn new(
        output_path: Option<String>,
        classification: Option<u8>,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;
        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);
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
    #[tracing::instrument(skip(self), fields(stage = "load_total"))]
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
        let (min_x, min_y, max_x, max_y) = self.bbox_projected()?;
        let mut t = TimingSpan::enter("load_total");
        let points =
            self.load_lidar_points_internal(&laz_urls, Some((min_x, min_y, max_x, max_y)))?;
        t.set_points(points.len());
        self.loaded_points = Some(points);
        Ok(())
    }

    /// Current WGS84 bbox transformed to the configured point-cloud CRS.
    pub(super) fn bbox_projected(&self) -> Result<(f64, f64, f64, f64)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set")?;
        let target_crs = format!("EPSG:{}", self.geo_core.get_epsg());
        let transformer = Proj::new_known_crs("EPSG:4326", &target_crs, None)
            .context("Failed to create coordinate transformer")?;
        let mut bounds = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for corner in [
            (bbox.min_x, bbox.min_y),
            (bbox.min_x, bbox.max_y),
            (bbox.max_x, bbox.min_y),
            (bbox.max_x, bbox.max_y),
        ] {
            let (x, y) = transformer
                .convert(corner)
                .context("Failed to transform bbox coordinates")?;
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
        Ok(bounds)
    }

    /// Set the native point-cloud CRS used for spatial filtering and raster output.
    ///
    /// Defaults to EPSG:2154 (mainland France); use EPSG:2975 for La Réunion.
    /// Points are not reprojected, so this must match the downloaded tiles.
    pub fn set_crs(&mut self, epsg: i32) {
        if self.geo_core.get_epsg() != epsg {
            self.geo_core.set_epsg(epsg);
            self.loaded_points = None;
        }
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
        let urls = self
            .list_path_laz
            .as_ref()
            .context("No COPC/LAZ URLs loaded. Call set_bbox() first.")?;
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

    #[tracing::instrument(skip(self, classification_list), fields(stage = "run"))]
    pub fn run(
        &mut self,
        file_name: Option<String>,
        classification_list: Option<Vec<u8>>,
        resolution: Option<f64>,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        let resolution = resolution.unwrap_or(1.0);
        self.ensure_points_loaded()?;
        let (min_x, min_y, max_x, max_y) = self.bbox_projected()?;
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
        let _t = TimingSpan::enter("geotiff");
        self.to_tif(&rasters, &output_file, write_out_file)
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

#[cfg(test)]
mod tests {
    use super::super::types::LidarPoint;
    use super::*;

    #[test]
    fn reunion_bbox_matches_native_tile_coordinates() -> Result<()> {
        let mut lidar = Lidar::new(None, None, None)?;
        lidar.set_crs(2975);
        lidar
            .geo_core
            .set_bbox(Some(BoundingBox::new(55.51, -21.285, 55.53, -21.265)));
        let (min_x, min_y, max_x, max_y) = lidar.bbox_projected()?;
        assert!(
            min_x > 345_000.0 && max_x < 348_000.0,
            "x: {min_x}..{max_x}"
        );
        assert!(
            min_y > 7_645_000.0 && max_y < 7_649_000.0,
            "y: {min_y}..{max_y}"
        );
        assert!(min_x < max_x && min_y < max_y);
        // All four projected corners must fit, not just the SW/NE diagonal.
        let proj = Proj::new_known_crs("EPSG:4326", "EPSG:2975", None)?;
        for corner in [
            (55.51, -21.285),
            (55.51, -21.265),
            (55.53, -21.285),
            (55.53, -21.265),
        ] {
            let (x, y) = proj.convert(corner)?;
            assert!((min_x..=max_x).contains(&x) && (min_y..=max_y).contains(&y));
        }
        Ok(())
    }

    #[test]
    fn reunion_run_filters_native_points_and_writes_projected_geotiff() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut lidar = Lidar::new(Some(dir.path().to_string_lossy().into_owned()), None, None)?;
        lidar.set_crs(2975);
        lidar
            .geo_core
            .set_bbox(Some(BoundingBox::new(55.51, -21.285, 55.53, -21.265)));
        let url = "https://example.test/reunion.laz".to_string();
        let cache_dir = dir.path().join(".cache/laz");
        std::fs::create_dir_all(&cache_dir)?;
        let tile_path = Lidar::cache_path_for_url(&cache_dir, &url);
        let (x, y) =
            Proj::new_known_crs("EPSG:4326", "EPSG:2975", None)?.convert((55.52, -21.275))?;
        let mut builder = las::Builder::default();
        // UTM northings exceed LAS i32 range with the default millimetre scale.
        builder.transforms.y.scale = 0.01;
        let mut writer = las::Writer::from_path(&tile_path, builder.into_header()?)?;
        for (point_x, z, class) in [(x, 100.0, 2), (x, 110.0, 5), (0.0, 999.0, 2)] {
            writer.write_point(las::Point {
                x: point_x,
                y,
                z,
                classification: las::point::Classification::new(class)?,
                ..Default::default()
            })?;
        }
        writer.close()?;
        lidar.list_path_laz = Some(vec![url]);
        let path = lidar.run(Some("reunion.tif".into()), None, Some(100.0), true)?;
        assert_eq!(
            lidar.loaded_points.as_ref().unwrap().len(),
            2,
            "only ROI points survive"
        );
        let dataset = gdal::Dataset::open(path)?;
        assert_eq!(dataset.spatial_ref()?.auth_code()?, 2975);
        let transform = dataset.geo_transform()?;
        assert_eq!(transform[1], 100.0);
        assert_eq!(transform[5], -100.0);
        let dsm = dataset.rasterband(1)?.read_band_as::<f64>()?;
        assert!(
            dsm.data().iter().any(|&z| (z - 110.0).abs() < 0.01),
            "DSM contains canopy height"
        );
        assert!(!dsm.data().contains(&999.0), "outside point was excluded");
        Ok(())
    }

    #[test]
    fn mainland_bbox_keeps_lambert93_default() -> Result<()> {
        let mut lidar = Lidar::new(None, None, None)?;
        lidar.geo_core.set_bbox(Some(BoundingBox::new(
            -1.152223, 46.183282, -1.149637, 46.185459,
        )));
        let (min_x, min_y, max_x, max_y) = lidar.bbox_projected()?;
        assert_eq!(lidar.geo_core.get_epsg(), 2154);
        assert!(
            min_x > 370_000.0 && max_x < 390_000.0,
            "x: {min_x}..{max_x}"
        );
        assert!(
            min_y > 6_570_000.0 && max_y < 6_590_000.0,
            "y: {min_y}..{max_y}"
        );
        assert!(min_x < max_x && min_y < max_y);
        Ok(())
    }

    #[test]
    fn changing_crs_invalidates_cached_points() -> Result<()> {
        let mut lidar = Lidar::new(None, None, None)?;
        lidar.loaded_points = Some(vec![LidarPoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            classification: 2,
        }]);
        lidar.set_crs(2154);
        assert!(
            lidar.loaded_points.is_some(),
            "unchanged CRS preserves cached points"
        );
        lidar.set_crs(2975);
        assert!(
            lidar.loaded_points.is_none(),
            "new CRS requires a new spatial filter"
        );
        Ok(())
    }
}
