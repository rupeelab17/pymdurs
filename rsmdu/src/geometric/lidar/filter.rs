//! Spatial bbox filtering of LiDAR points.

use super::spatial::{QuadtreeSpatialIndex, SpatialGridIndex};
use super::types::{Lidar, LidarPoint};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

impl Lidar {

    pub(super) fn filter_points_with_spatial_index(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();
        if point_count < 10_000 {
            return points
                .iter()
                .copied()
                .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
                .collect();
        }
        tracing::info!("  Building spatial index for {} points...", point_count);
        let start = std::time::Instant::now();
        let query_area = (x_max - x_min) * (y_max - y_min);
        let sample_size = (point_count / 100).max(100).min(point_count);
        let step = point_count / sample_size;
        let mut data_min_x = f64::INFINITY;
        let mut data_min_y = f64::INFINITY;
        let mut data_max_x = f64::NEG_INFINITY;
        let mut data_max_y = f64::NEG_INFINITY;
        for i in (0..point_count).step_by(step.max(1)) {
            let p = &points[i];
            data_min_x = data_min_x.min(p.x);
            data_min_y = data_min_y.min(p.y);
            data_max_x = data_max_x.max(p.x);
            data_max_y = data_max_y.max(p.y);
        }
        let data_area = (data_max_x - data_min_x) * (data_max_y - data_min_y);
        let selectivity = if data_area > 0.0 {
            query_area / data_area
        } else {
            1.0
        };
        tracing::info!("  📐 Query selectivity: {:.1}%", selectivity * 100.0);
        let result = if selectivity > 0.5 || point_count < 100_000 {
            let cell_size = ((data_max_x - data_min_x) / 100.0)
                .max((data_max_y - data_min_y) / 100.0)
                .max(10.0);
            let grid_index = SpatialGridIndex::build_from_points(points, cell_size);
            tracing::info!("  {}", grid_index.stats());
            let candidate_indices = grid_index.query_bbox(x_min, y_min, x_max, y_max);
            tracing::info!(
                "  🔍 Grid query returned {} candidates",
                candidate_indices.len()
            );
            candidate_indices
                .into_iter()
                .filter_map(|i| {
                    let p = points[i];
                    if p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            let quadtree = QuadtreeSpatialIndex::build(points);
            tracing::info!("  {}", quadtree.stats());
            let candidate_indices = quadtree.query_bbox(x_min, y_min, x_max, y_max);
            tracing::info!(
                "  🔍 Quadtree query returned {} candidates",
                candidate_indices.len()
            );
            candidate_indices
                .into_iter()
                .filter_map(|i| {
                    let p = points[i];
                    if p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect()
        };
        tracing::info!(
            "  Spatial indexing and query took {:.2}s",
            start.elapsed().as_secs_f64()
        );
        result
    }

    #[cfg(feature = "rayon")]
    pub(super) fn filter_points_with_spatial_index_parallel(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();
        if point_count < 1_000_000 {
            return Self::filter_points_with_spatial_index(points, x_min, y_min, x_max, y_max);
        }
        tracing::info!(
            "  🚀 Using parallel spatial indexing for {} points...",
            point_count
        );
        let start = std::time::Instant::now();
        let chunk_size = 500_000;
        let result: Vec<LidarPoint> = points
            .par_chunks(chunk_size)
            .map(|chunk| Self::filter_points_with_spatial_index(chunk, x_min, y_min, x_max, y_max))
            .reduce(Vec::new, |mut acc, mut part| {
                if acc.is_empty() {
                    return part;
                }
                acc.reserve(part.len());
                acc.append(&mut part);
                acc
            });
        tracing::info!(
            "  Parallel spatial filtering took {:.2}s",
            start.elapsed().as_secs_f64()
        );
        result
    }
}
