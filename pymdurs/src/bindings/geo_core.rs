use rsmdu::geo_core::GeoCore;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::bindings::bounding_box::PyBoundingBox;

/// GeoCore Python binding
#[gen_stub_pyclass(module = "pymdurs")]
#[pyclass]
pub struct PyGeoCore {
    pub(crate) inner: GeoCore, // pub(crate) allows access from other modules in the same crate
}

#[gen_stub_pymethods]
#[pymethods]
impl PyGeoCore {
    #[new]
    #[pyo3(signature = (epsg = 2154))]
    fn new(epsg: i32) -> Self {
        PyGeoCore {
            inner: GeoCore::new(epsg),
        }
    }

    #[getter]
    fn epsg(&self) -> i32 {
        self.inner.get_epsg()
    }

    #[setter]
    fn set_epsg(&mut self, epsg: i32) {
        self.inner.set_epsg(epsg);
    }

    #[getter]
    fn bbox(&self) -> Option<PyBoundingBox> {
        self.inner
            .get_bbox()
            .map(|bbox| PyBoundingBox { inner: bbox })
    }

    #[setter]
    fn set_bbox(&mut self, bbox: Option<PyBoundingBox>) {
        self.inner.set_bbox(bbox.map(|pb| pb.inner));
    }

    #[getter]
    fn output_path(&self) -> Option<String> {
        self.inner.get_output_path().cloned()
    }

    #[setter]
    fn set_output_path(&mut self, output_path: Option<String>) {
        self.inner.set_output_path(output_path);
    }
}
