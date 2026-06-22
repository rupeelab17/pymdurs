use rsmdu::geometric::dem::Dem;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::bindings::geo_core::PyGeoCore;

/// Dem Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyDem {
    inner: Dem,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyDem {
    #[new]
    #[pyo3(signature = (output_path = None))]
    fn new(output_path: Option<String>) -> PyResult<Self> {
        match Dem::new(output_path) {
            Ok(dem) => Ok(PyDem { inner: dem }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Dem: {}",
                e
            ))),
        }
    }

    /// Set bounding box
    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.inner.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Set CRS
    fn set_crs(&mut self, epsg: i32) {
        self.inner.set_crs(epsg);
    }

    /// Run DEM processing
    #[pyo3(signature = (shape = None))]
    #[gen_stub(override_return_type(type_repr = "Self"))]
    fn run(mut slf: PyRefMut<Self>, shape: Option<(u32, u32)>) -> PyResult<PyRefMut<Self>> {
        // Use run_internal which works on &mut self
        slf.inner
            .run_internal(shape)
            .map_err(|e| PyValueError::new_err(format!("Failed to run DEM: {}", e)))?;
        Ok(slf)
    }

    /// Get path to saved TIFF file
    fn get_path_save_tiff(&self) -> String {
        self.inner
            .get_path_save_tiff()
            .to_string_lossy()
            .to_string()
    }

    /// Get path to mask shapefile
    fn get_path_save_mask(&self) -> String {
        self.inner
            .get_path_save_mask()
            .to_string_lossy()
            .to_string()
    }

    /// Get GeoCore instance
    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
