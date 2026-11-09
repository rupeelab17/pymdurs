//! Core LiDAR types.

use std::path::PathBuf;

use crate::geo_core::GeoCore;

#[cfg(feature = "progressbar")]
use indicatif::ProgressStyle;

#[cfg(feature = "progressbar")]
pub(super) fn progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {percent} {msg}")
        .unwrap()
        .progress_chars("##-")
}


pub struct Lidar {
    pub geo_core: GeoCore,
    pub(super) output_path: PathBuf,
    pub(super) classification: Option<u8>,
    pub(super) list_path_laz: Option<Vec<String>>,
    pub(super) loaded_points: Option<Vec<LidarPoint>>,
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
