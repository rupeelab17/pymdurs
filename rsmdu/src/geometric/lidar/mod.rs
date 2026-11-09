//! LiDAR HD (IGN) — download, COPC/LAZ load, DSM/DTM/CHM rasterization.

mod api;
mod copc;
mod download;
mod io;
mod load;
mod load_copc;
mod filter;
mod process;
mod raster;
mod spatial;
mod types;

#[cfg(feature = "gpu")]
mod gpu;

pub use spatial::QuadtreeSpatialIndex;
pub use types::{DtmFill, Lidar};

#[cfg(test)]
mod tests;
