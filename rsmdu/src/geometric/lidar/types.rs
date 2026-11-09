//! Core LiDAR types.

use std::path::PathBuf;
use std::time::Duration;

use crate::geo_core::GeoCore;

/// Structured stage timing for `Lidar::run` profiling (cache cold/hot).
pub(super) fn log_timing(stage: &str, elapsed: Duration, points: Option<usize>) {
    log_timing_backend(stage, elapsed, points, None);
}

/// Like [`log_timing`], with optional `backend=cpu|gpu`.
pub(super) fn log_timing_backend(
    stage: &str,
    elapsed: Duration,
    points: Option<usize>,
    backend: Option<&str>,
) {
    let ms = elapsed.as_secs_f64() * 1000.0;
    let backend = backend
        .map(|b| format!(" backend={b}"))
        .unwrap_or_default();
    match points {
        Some(n) => eprintln!("lidar_timing stage={stage} ms={ms:.2} points={n}{backend}"),
        None => eprintln!("lidar_timing stage={stage} ms={ms:.2}{backend}"),
    }
}

#[cfg(feature = "progressbar")]
use indicatif::ProgressStyle;

#[cfg(feature = "progressbar")]
pub(super) fn progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {percent} {msg}")
        .unwrap()
        .progress_chars("##-")
}


/// How DTM cells without ground points (under buildings, dense canopy…) are filled
/// before computing the CHM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DtmFill {
    /// Legacy: one 3×3 pass taking the min ground neighbour; cells with no ground
    /// neighbour get 0.0 (CHM then equals the absolute DSM there).
    #[default]
    Neighbor3x3,
    /// Height of the nearest ground cell (Jump Flooding): no 0.0 holes.
    Nearest,
}

pub struct Lidar {
    pub geo_core: GeoCore,
    pub(super) output_path: PathBuf,
    pub(super) classification: Option<u8>,
    pub(super) list_path_laz: Option<Vec<String>>,
    pub(super) loaded_points: Option<Vec<LidarPoint>>,
    pub(super) dtm_fill: DtmFill,
}

#[derive(Debug, Clone)]
pub(crate) struct LidarPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) classification: u8,
}

#[derive(Debug)]
pub(super) struct ProcessedRasters {
    pub(super) dsm: Vec<Vec<f64>>,
    pub(super) dtm: Vec<Vec<f64>>,
    pub(super) chm: Vec<Vec<f64>>,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) transform: [f64; 6],
}

pub(super) fn classification_to_u8(c: &las::point::Classification) -> u8 {
    u8::from(*c)
}
