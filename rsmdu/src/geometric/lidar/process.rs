//! DSM/DTM/CHM rasterization and GeoTIFF write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::raster::rasterize_max_z_cpu;
use super::types::{Lidar, LidarPoint, ProcessedRasters};

#[cfg(feature = "gpu")]
use super::gpu::{rasterize_max_z_gpu, GPU_MIN_POINTS};

impl Lidar {

    pub(super) fn process_lidar_points(
        &self,
        points: Vec<LidarPoint>,
        bbox: (f64, f64, f64, f64),
        classification_list: Option<Vec<u8>>,
        resolution: f64,
    ) -> Result<ProcessedRasters> {
        let (x_min, y_min, x_max, y_max) = bbox;
        let filtered_points: Vec<LidarPoint> = points
            .into_iter()
            .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
            .collect();
        println!("Filtered {} points within bbox", filtered_points.len());
        let filtered_points: Vec<LidarPoint> = if let Some(ref class_list) = classification_list {
            filtered_points
                .into_iter()
                .filter(|p| class_list.contains(&p.classification))
                .collect()
        } else {
            filtered_points
        };
        println!(
            "After classification filter: {} points",
            filtered_points.len()
        );
        let width = ((x_max - x_min) / resolution).ceil() as usize;
        let height = ((y_max - y_min) / resolution).ceil() as usize;
        println!(
            "Grid dimensions: {}x{} (resolution: {}m)",
            width, height, resolution
        );
        let (dsm, dtm) = {
            #[cfg(feature = "gpu")]
            {
                if filtered_points.len() >= GPU_MIN_POINTS {
                    match rasterize_max_z_gpu(
                        &filtered_points,
                        bbox,
                        resolution,
                        width,
                        height,
                    ) {
                        Ok(grids) => {
                            println!(
                                "  GPU max-Z rasterize ({} points)",
                                filtered_points.len()
                            );
                            grids
                        }
                        Err(e) => {
                            println!("  GPU rasterize fallback to CPU: {e}");
                            rasterize_max_z_cpu(
                                &filtered_points,
                                x_min,
                                y_max,
                                resolution,
                                width,
                                height,
                            )
                        }
                    }
                } else {
                    rasterize_max_z_cpu(
                        &filtered_points,
                        x_min,
                        y_max,
                        resolution,
                        width,
                        height,
                    )
                }
            }
            #[cfg(not(feature = "gpu"))]
            {
                rasterize_max_z_cpu(
                    &filtered_points,
                    x_min,
                    y_max,
                    resolution,
                    width,
                    height,
                )
            }
        };
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
