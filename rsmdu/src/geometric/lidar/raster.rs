//! CPU DSM/DTM max-Z rasterization.

use super::types::LidarPoint;

/// CPU max-Z binning into DSM (all points) and DTM (class 2).
pub(super) fn rasterize_max_z_cpu(
    points: &[LidarPoint],
    x_min: f64,
    y_max: f64,
    resolution: f64,
    width: usize,
    height: usize,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let mut dsm = vec![vec![f64::NEG_INFINITY; width]; height];
    let mut dtm = vec![vec![f64::NEG_INFINITY; width]; height];
    for point in points {
        let col = ((point.x - x_min) / resolution).floor() as usize;
        let row = ((y_max - point.y) / resolution).floor() as usize;
        if col < width && row < height {
            if point.z > dsm[row][col] || dsm[row][col] == f64::NEG_INFINITY {
                dsm[row][col] = point.z;
            }
            if point.classification == 2
                && (point.z > dtm[row][col] || dtm[row][col] == f64::NEG_INFINITY)
            {
                dtm[row][col] = point.z;
            }
        }
    }
    (dsm, dtm)
}
