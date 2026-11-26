// Python bindings module
// Each file contains one or more PyO3 #[pyclass] definitions

pub mod bounding_box;
pub mod building;
pub mod cosia;
pub mod dem;
pub mod geo_core;
pub mod ign_layer;
pub mod lcz;
pub mod lidar;
pub mod lidar_raster;
pub mod logging;

pub mod rnb;
pub mod vegetation;
pub mod water;
pub mod wind;

// Re-export all bindings for convenience
pub use bounding_box::PyBoundingBox;
pub use building::PyBuilding;
pub use cosia::PyCosia;
pub use dem::PyDem;
pub use geo_core::PyGeoCore;
pub use ign_layer::{PyCadastre, PyIris, PyRoad};
pub use lcz::PyLcz;
pub use lidar::PyLidar;
pub use lidar_raster::{PyMnh, PyMns, PyMnt};
pub use logging::init_logging;
pub use rnb::PyRnb;
pub use vegetation::PyVegetation;
pub use water::PyWater;
pub use wind::{PyWindConfig, PyWindField};
