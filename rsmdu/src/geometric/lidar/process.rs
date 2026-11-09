//! DSM/DTM/CHM rasterization and GeoTIFF write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::raster::{chm_cpu, fill_dtm_cpu, rasterize_max_z_cpu, to_rows, ClassMask};
use super::types::{log_timing_backend, DtmFill, Lidar, LidarPoint, ProcessedRasters};

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
                Err(e) => println!("  GPU raster fallback to CPU: {e}"),
            }
        } else {
            println!(
                "  CPU raster ({} points, below GPU_MIN_POINTS)",
                points.len()
            );
        }
    }

    let (x_min, _y_min, _x_max, y_max) = bbox;
    let t = Instant::now();
    let (dsm, dtm) = rasterize_max_z_cpu(points, mask, x_min, y_max, resolution, width, height);
    log_timing_backend("rasterize", t.elapsed(), Some(points.len()), Some("cpu"));
    let t = Instant::now();
    let dtm_filled = fill_dtm_cpu(&dtm, width, height, fill);
    let chm = chm_cpu(&dsm, &dtm_filled);
    log_timing_backend("fill_chm", t.elapsed(), None, Some("cpu"));
    (dsm, dtm_filled, chm, "cpu")
}

impl Lidar {
    /// Rasterize already bbox-filtered points into DSM/DTM/CHM.
    ///
    /// The classification filter is applied inside the binning (CPU loop or GPU kernel),
    /// so the point cloud is never copied.
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
        println!(
            "Grid dimensions: {}x{} (resolution: {}m, DTM fill: {:?})",
            width, height, resolution, self.dtm_fill
        );

        let t = Instant::now();
        let (dsm, dtm, chm, backend) =
            compute_rasters(points, bbox, &mask, resolution, width, height, self.dtm_fill);
        log_timing_backend("raster_total", t.elapsed(), Some(points.len()), Some(backend));

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
            println!("GeoTIFF saved to: {:?}", output_path);
        }
        Ok(output_path.to_path_buf())
    }
}
