//! Urban wind field (Röckle model) for thermal/UTCI pipeline.
//!
//! Requires `gdal`, `rayon`, and `ndarray` features.

#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub mod wind_field;

#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub use wind_field::*;
