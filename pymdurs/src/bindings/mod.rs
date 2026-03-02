// Python bindings module
// Each file contains one or more PyO3 #[pyclass] definitions

pub mod bounding_box;
pub mod building;
pub mod cadastre;
pub mod cosia;
pub mod dem;
pub mod geo_core;
pub mod iris;
pub mod lcz;
pub mod lidar;
pub mod rnb;
pub mod road;
pub mod vegetation;
pub mod water;

// Re-export all bindings for convenience
pub use bounding_box::PyBoundingBox;
pub use building::PyBuilding;
pub use cadastre::PyCadastre;
pub use cosia::PyCosia;
pub use dem::PyDem;
pub use geo_core::PyGeoCore;
pub use iris::PyIris;
pub use lcz::PyLcz;
pub use lidar::PyLidar;
pub use rnb::PyRnb;
pub use road::PyRoad;
pub use vegetation::PyVegetation;
pub use water::PyWater;
