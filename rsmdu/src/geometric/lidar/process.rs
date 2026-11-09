//! DSM/DTM/CHM rasterization and GeoTIFF write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::raster::rasterize_max_z_cpu;
use super::types::{log_timing, log_timing_backend, Lidar, LidarPoint, ProcessedRasters};

#[cfg(feature = "gpu")]
use super::gpu::{rasterize_max_z_gpu, GPU_MIN_POINTS};

impl Lidar {
    /// Rasterize already bbox-filtered points into DSM/DTM/CHM.
    ///
    /// Bbox filtering is done at load time; only optional classification filter remains here.
    pub(super) fn process_lidar_points(
        &self,
        points: &[LidarPoint],
        bbox: (f64, f64, f64, f64),
        classification_list: Option<Vec<u8>>,
        resolution: f64,
    ) -> Result<ProcessedRasters> {
        let (x_min, y_min, x_max, y_max) = bbox;

        let t_filter = Instant::now();
        let owned: Option<Vec<LidarPoint>> = classification_list.as_ref().map(|class_list| {
            points
                .iter()
                .filter(|p| class_list.contains(&p.classification))
                .cloned()
                .collect()
        });
        let pts: &[LidarPoint] = owned.as_deref().unwrap_or(points);
        log_timing("filter", t_filter.elapsed(), Some(pts.len()));
        println!("After classification filter: {} points", pts.len());

        let width = ((x_max - x_min) / resolution).ceil() as usize;
        let height = ((y_max - y_min) / resolution).ceil() as usize;
        println!(
            "Grid dimensions: {}x{} (resolution: {}m)",
            width, height, resolution
        );

        let t_rast = Instant::now();
        let ((dsm, dtm), rast_backend) = {
            #[cfg(feature = "gpu")]
            {
                if pts.len() >= GPU_MIN_POINTS {
                    match rasterize_max_z_gpu(pts, bbox, resolution, width, height) {
                        Ok(grids) => {
                            println!("  GPU max-Z rasterize ({} points)", pts.len());
                            (grids, "gpu")
                        }
                        Err(e) => {
                            println!("  GPU rasterize fallback to CPU: {e}");
                            (
                                rasterize_max_z_cpu(pts, x_min, y_max, resolution, width, height),
                                "cpu",
                            )
                        }
                    }
                } else {
                    println!(
                        "  CPU max-Z rasterize ({} points, below GPU_MIN_POINTS)",
                        pts.len()
                    );
                    (
                        rasterize_max_z_cpu(pts, x_min, y_max, resolution, width, height),
                        "cpu",
                    )
                }
            }
            #[cfg(not(feature = "gpu"))]
            {
                println!("  CPU max-Z rasterize ({} points)", pts.len());
                (
                    rasterize_max_z_cpu(pts, x_min, y_max, resolution, width, height),
                    "cpu",
                )
            }
        };
        log_timing_backend(
            "rasterize",
            t_rast.elapsed(),
            Some(pts.len()),
            Some(rast_backend),
        );

        let t_fill = Instant::now();
        let mut dtm_filled = dtm.clone();
        for row in 0..height {
            for col in 0..width {
                if dtm_filled[row][col] == f64::NEG_INFINITY {
                    let mut min_neighbor = f64::INFINITY;
                    for dr in [-1, 0, 1] {
                        for dc in [-1, 0, 1] {
                            let r = row as i32 + dr;
                            let c = col as i32 + dc;
                            if r >= 0 && r < height as i32 && c >= 0 && c < width as i32 {
                                let val = dtm[r as usize][c as usize];
                                if val != f64::NEG_INFINITY && val < min_neighbor {
                                    min_neighbor = val;
                                }
                            }
                        }
                    }
                    dtm_filled[row][col] = if min_neighbor != f64::INFINITY {
                        min_neighbor
                    } else {
                        0.0
                    };
                }
            }
        }
        let mut chm = vec![vec![0.0; width]; height];
        for row in 0..height {
            for col in 0..width {
                if dsm[row][col] != f64::NEG_INFINITY && dtm_filled[row][col] != f64::NEG_INFINITY {
                    chm[row][col] = (dsm[row][col] - dtm_filled[row][col]).max(0.0);
                }
            }
        }
        log_timing("fill_chm", t_fill.elapsed(), None);

        Ok(ProcessedRasters {
            dsm,
            dtm: dtm_filled,
            chm,
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
