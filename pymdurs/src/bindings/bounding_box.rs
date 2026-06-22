use rsmdu::geo_core::BoundingBox;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

/// BoundingBox Python binding
#[gen_stub_pyclass(module = "pymdurs")]
#[pyclass]
#[derive(Clone)]
pub struct PyBoundingBox {
    pub(crate) inner: BoundingBox, // pub(crate) allows access from other modules in the same crate
}

#[gen_stub_pymethods]
#[pymethods]
impl PyBoundingBox {
    #[new]
    fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        PyBoundingBox {
            inner: BoundingBox::new(min_x, min_y, max_x, max_y),
        }
    }

    #[getter]
    fn min_x(&self) -> f64 {
        self.inner.min_x
    }

    #[getter]
    fn min_y(&self) -> f64 {
        self.inner.min_y
    }

    #[getter]
    fn max_x(&self) -> f64 {
        self.inner.max_x
    }

    #[getter]
    fn max_y(&self) -> f64 {
        self.inner.max_y
    }

    /// Transform bounding box to another CRS
    fn transform(&self, from_epsg: i32, to_epsg: i32) -> PyResult<Self> {
        match self.inner.transform(from_epsg, to_epsg) {
            Ok(bbox) => Ok(PyBoundingBox { inner: bbox }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Transformation failed: {}",
                e
            ))),
        }
    }
}
