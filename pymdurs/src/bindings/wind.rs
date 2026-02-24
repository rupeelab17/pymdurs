//! Python bindings for rsmdu::thermal::WindField (Röckle urban wind).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::path::Path;

use rsmdu::thermal::{WindConfig, WindField};

/// WindConfig: reference speed, direction, height, resolution.
#[pyclass]
pub struct PyWindConfig {
    #[pyo3(get, set)]
    pub wind_speed_ref: f64,
    #[pyo3(get, set)]
    pub wind_direction: f64,
    #[pyo3(get, set)]
    pub z_ref: f64,
    #[pyo3(get, set)]
    pub resolution_m: f64,
}

#[pymethods]
impl PyWindConfig {
    #[new]
    #[pyo3(signature = (wind_speed_ref=3.5, wind_direction=225.0, z_ref=10.0, resolution_m=2.0))]
    fn new(
        wind_speed_ref: f64,
        wind_direction: f64,
        z_ref: f64,
        resolution_m: f64,
    ) -> Self {
        PyWindConfig {
            wind_speed_ref,
            wind_direction,
            z_ref,
            resolution_m,
        }
    }
}

/// WindField: Röckle-based urban wind solver (outputs wind_speed.tif, wind_direction.tif).
#[pyclass]
pub struct PyWindField {
    inner: WindField,
}

#[pymethods]
impl PyWindField {
    #[new]
    #[pyo3(signature = (output_path = None))]
    fn new(output_path: Option<String>) -> PyResult<Self> {
        WindField::new(output_path)
            .map(|w| PyWindField { inner: w })
            .map_err(|e| PyValueError::new_err(format!("Failed to create WindField: {}", e)))
    }

    /// Set bounding box (min_x, min_y, max_x, max_y) in project CRS.
    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.inner.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Run wind field: requires DEM and DSM GeoTIFF paths and a BuildingCollection.
    /// Writes wind_speed.tif and wind_direction.tif to the output path.
    fn run(
        &self,
        config: &PyWindConfig,
        dem_path: &str,
        dsm_path: &str,
        buildings: &crate::bindings::building::PyBuilding,
    ) -> PyResult<(String, String)> {
        let cfg = WindConfig {
            wind_speed_ref: config.wind_speed_ref,
            wind_direction: config.wind_direction,
            z_ref: config.z_ref,
            resolution_m: config.resolution_m,
        };
        let result = self
            .inner
            .run(
                cfg,
                Path::new(dem_path),
                Path::new(dsm_path),
                &buildings.inner,
            )
            .map_err(|e| PyValueError::new_err(format!("WindField run failed: {}", e)))?;
        Ok((
            result.wind_speed_path.to_string_lossy().to_string(),
            result.wind_direction_path.to_string_lossy().to_string(),
        ))
    }

    /// Get GeoCore instance
    #[getter]
    fn geo_core(&self) -> crate::bindings::geo_core::PyGeoCore {
        crate::bindings::geo_core::PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
