use rsmdu::geometric::iris::Iris;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::bindings::geo_core::PyGeoCore;

/// Iris Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyIris {
    inner: Iris,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyIris {
    #[new]
    #[pyo3(signature = (output_path = None))]
    fn new(output_path: Option<String>) -> PyResult<Self> {
        match Iris::new(output_path) {
            Ok(iris) => Ok(PyIris { inner: iris }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Iris: {}",
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
            .map_err(|e| PyValueError::new_err(format!("Failed to run Iris: {}", e)))?;
        Ok(slf)
    }

    #[gen_stub(override_return_type(type_repr = "dict[str, typing.Any]", imports = ("typing",)))]
    fn get_geojson(&self, py: Python) -> PyResult<Py<PyAny>> {
        match self.inner.get_geojson() {
            Some(geojson) => {
                let json_str = geojson.to_string();
                let json = py.import("json")?;
                let geojson_dict: pyo3::Bound<PyAny> = json.call_method1("loads", (json_str,))?;
                Ok(geojson_dict.unbind())
            }
            None => Err(PyValueError::new_err(
                "No GeoJSON data available. Call run() first.",
            )),
        }
    }

    #[pyo3(signature = (name = None))]
    fn to_geojson(&self, name: Option<&str>) -> PyResult<()> {
        self.inner
            .to_geojson(name)
            .map_err(|e| PyValueError::new_err(format!("Failed to save GeoJSON: {}", e)))
    }

    fn get_output_path(&self) -> String {
        self.inner.get_output_path().to_string_lossy().to_string()
    }

    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
