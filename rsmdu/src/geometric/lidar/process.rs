//! DSM/DTM/CHM rasterization and GeoTIFF write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::raster::{chm_cpu, fill_dtm_cpu, rasterize_max_z_cpu, to_rows, ClassMask};
use super::types::{DtmFill, Lidar, LidarPoint, ProcessedRasters, TimingSpan};

#[cfg(feature = "gpu")]
use super::gpu::{rasterize_fill_chm_gpu, GPU_MIN_POINTS};

/// DSM, filled DTM and CHM (flat, row-major) + backend used.
pub(super) fn compute_rasters(
    points: &[LidarPoint],
    bbox: (f64, f64, f64, f64),
    mask: &ClassMask,
    resolution: f64,
    width: usize,
    height: usize,
    fill: DtmFill,
) -> (Vec<f64>, Vec<f64>, Vec<f64>, &'static str) {
    #[cfg(feature = "gpu")]
    {
        if points.len() >= GPU_MIN_POINTS {
            match rasterize_fill_chm_gpu(points, bbox, mask, resolution, width, height, fill) {
                Ok(r) => {
                    let widen = |v: Vec<f32>| v.into_iter().map(f64::from).collect::<Vec<f64>>();
                    return (widen(r.dsm), widen(r.dtm), widen(r.chm), "gpu");
                }
                Err(e) => tracing::info!("  GPU raster fallback to CPU: {e}"),
            }
        } else {
            tracing::info!(
                "  CPU raster ({} points, below GPU_MIN_POINTS)",
                points.len()
            );
        }
    }

    let (x_min, _y_min, _x_max, y_max) = bbox;
    let (dsm, dtm) = {
        let mut t = TimingSpan::enter("rasterize");
        t.set_points(points.len());
        t.set_backend("cpu");
        rasterize_max_z_cpu(points, mask, x_min, y_max, resolution, width, height)
    };
    let (dtm_filled, chm) = {
        let mut t = TimingSpan::enter("fill_chm");
        t.set_backend("cpu");
        let filled = fill_dtm_cpu(&dtm, width, height, fill);
        let chm = chm_cpu(&dsm, &filled);
        (filled, chm)
    };
    (dsm, dtm_filled, chm, "cpu")
}

impl Lidar {
    /// Rasterize already bbox-filtered points into DSM/DTM/CHM.
    ///
    /// The classification filter is applied inside the binning (CPU loop or GPU kernel),
    /// so the point cloud is never copied.
    #[tracing::instrument(skip(self, points, classification_list), fields(stage = "raster_total"))]
    pub(super) fn process_lidar_points(
        &self,
        points: &[LidarPoint],
        bbox: (f64, f64, f64, f64),
        classification_list: Option<Vec<u8>>,
        resolution: f64,
    ) -> Result<ProcessedRasters> {
        let (x_min, y_min, x_max, y_max) = bbox;
        let mask = ClassMask::from_list(classification_list.as_deref());

        let width = ((x_max - x_min) / resolution).ceil() as usize;
        let height = ((y_max - y_min) / resolution).ceil() as usize;
        tracing::info!(
            "Grid dimensions: {}x{} (resolution: {}m, DTM fill: {:?})",
            width, height, resolution, self.dtm_fill
        );

        let mut t = TimingSpan::enter("raster_total");
        t.set_points(points.len());
        let (dsm, dtm, chm, backend) =
            compute_rasters(points, bbox, &mask, resolution, width, height, self.dtm_fill);
        t.set_backend(backend);

        Ok(ProcessedRasters {
            dsm: to_rows(dsm, width, height),
            dtm: to_rows(dtm, width, height),
            chm: to_rows(chm, width, height),
            width,
            height,
            transform: [x_min, resolution, 0.0, y_max, 0.0, -resolution],
        })
    }

    pub(super) fn to_tif(
        &self,
        rasters: &ProcessedRasters,
        output_path: &Path,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        use gdal::raster::Buffer;
        use gdal::spatial_ref::SpatialRef;
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }
        if write_out_file {
            let driver = gdal::DriverManager::get_driver_by_name("GTiff")
                .context("Failed to get GTiff driver")?;
            let mut dataset = driver
                .create_with_band_type::<f64, _>(output_path, rasters.width, rasters.height, 3)
                .context("Failed to create GeoTIFF dataset")?;
            dataset
                .set_geo_transform(&rasters.transform)
                .context("Failed to set geotransform")?;
            let srs = SpatialRef::from_epsg(self.geo_core.get_epsg() as u32)
                .context("Failed to create spatial reference")?;
            dataset
                .set_spatial_ref(&srs)
                .context("Failed to set spatial reference")?;
            for (band_idx, (grid, nodata)) in [
                (&rasters.dsm, f64::NAN),
                (&rasters.dtm, f64::NAN),
                (&rasters.chm, 0.0f64),
            ]
            .iter()
            .enumerate()
            {
                let mut band = dataset
                    .rasterband(band_idx + 1)
                    .context(format!("Failed to get band {}", band_idx + 1))?;
                let data: Vec<f64> = grid
                    .iter()
                    .flat_map(|row| {
                        row.iter().map(|&val| {
                            if val == f64::NEG_INFINITY {
                                *nodata
                            } else {
                                val
                            }
                        })
                    })
                    .collect();
                let mut buffer = Buffer::new((rasters.width, rasters.height), data);
                band.write((0, 0), (rasters.width, rasters.height), &mut buffer)
                    .context(format!("Failed to write band {}", band_idx + 1))?;
                band.set_no_data_value(Some(*nodata))
                    .context(format!("Failed to set nodata for band {}", band_idx + 1))?;
            }
            tracing::info!("GeoTIFF saved to: {:?}", output_path);
        }
        Ok(output_path.to_path_buf())
    }
}
