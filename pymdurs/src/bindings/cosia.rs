use rsmdu::geometric::cosia::Cosia;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::bindings::geo_core::PyGeoCore;

/// Cosia Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyCosia {
    inner: Cosia,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyCosia {
    #[new]
    #[pyo3(signature = (output_path = None, template_raster_path = None))]
    fn new(
        output_path: Option<String>,
        template_raster_path: Option<String>,
    ) -> PyResult<Self> {
        match Cosia::new(output_path, template_raster_path) {
            Ok(cosia) => Ok(PyCosia { inner: cosia }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Cosia: {}",
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

    /// Run Cosia processing: download from IGN API
    #[gen_stub(override_return_type(type_repr = "Self"))]
    fn run_ign(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
        // Use run_ign_internal which works on &mut self
        slf.inner
            .run_ign_internal()
            .map_err(|e| PyValueError::new_err(format!("Failed to run Cosia: {}", e)))?;
        Ok(slf)
    }

    /// Get path to saved TIFF file
    fn get_path_save_tiff(&self) -> String {
        self.inner
            .get_path_save_tiff()
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

