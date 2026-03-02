//! Urban wind field (Röckle model) for thermal/UTCI pipeline.
//!
//! Requires `gdal`, `rayon`, and `ndarray` features.

#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub mod mass_consistent_solver;
#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub mod vegetation;
#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub mod wind_field;

#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub use mass_consistent_solver::solve_mass_consistent;
#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub use vegetation::{VegetationCollection, VegetationPatch, vegetation_wind_factor};
#[cfg(all(feature = "gdal", feature = "rayon", feature = "ndarray"))]
pub use wind_field::*;
