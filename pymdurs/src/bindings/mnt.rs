use rsmdu::geometric::mnt::Mnt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::bindings::geo_core::PyGeoCore;

/// MNT (LiDAR HD) Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyMnt {
    inner: Mnt,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyMnt {
    #[new]
    #[pyo3(signature = (output_path = None))]
    fn new(output_path: Option<String>) -> PyResult<Self> {
        match Mnt::new(output_path) {
            Ok(mnt) => Ok(PyMnt { inner: mnt }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Mnt: {}",
                e
            ))),
        }
    }

    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.inner.set_bbox(min_x, min_y, max_x, max_y);
    }

    fn set_crs(&mut self, epsg: i32) {
        self.inner.set_crs(epsg);
    }

    #[gen_stub(override_return_type(type_repr = "Self"))]
    fn run(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
        slf.inner
            .run_internal()
            .map_err(|e| PyValueError::new_err(format!("Failed to run MNT: {}", e)))?;
        Ok(slf)
    }

    fn get_path_save_tiff(&self) -> String {
        self.inner
            .get_path_save_tiff()
            .to_string_lossy()
            .to_string()
    }

    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
