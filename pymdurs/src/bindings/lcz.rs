use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_stub_gen::derive::*;
use rsmdu::geometric::lcz::Lcz;

use crate::bindings::geo_core::PyGeoCore;

/// Lcz Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyLcz {
    inner: Lcz,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyLcz {
    #[new]
    #[pyo3(signature = (filepath_shp = None, output_path = None, set_crs = None))]
    fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
    ) -> PyResult<Self> {
        match Lcz::new(filepath_shp, output_path, set_crs) {
            Ok(lcz) => Ok(PyLcz { inner: lcz }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Lcz: {}",
                e
            ))),
        }
    }

    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.inner.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Set CRS
    fn set_crs(&mut self, epsg: i32) {
        self.inner.geo_core.set_epsg(epsg);
    }

    #[pyo3(signature = (zipfile_url = None))]
    #[gen_stub(override_return_type(type_repr = "Self"))]
    fn run(mut slf: PyRefMut<Self>, zipfile_url: Option<String>) -> PyResult<PyRefMut<Self>> {
        slf.inner
            .run(zipfile_url.as_deref())
            .map_err(|e| PyValueError::new_err(format!("Failed to run Lcz: {}", e)))?;
        Ok(slf)
    }

    #[gen_stub(override_return_type(type_repr = "dict[str, typing.Any] | None", imports = ("typing",)))]
    fn geojson(&self, py: Python) -> PyResult<Option<Py<PyAny>>> {
        match self.inner.geojson() {
            Some(geojson) => {
                let json_str = geojson.to_string();
                let json = py.import("json")?;
                let geojson_dict: pyo3::Bound<PyAny> = json.call_method1("loads", (json_str,))?;
                Ok(Some(geojson_dict.unbind()))
            }
            None => {
                // LCZ processing is not yet fully implemented
                // Return None instead of raising an error
                Ok(None)
            }
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

    #[gen_stub(override_return_type(type_repr = "dict[int, dict[str, str]]"))]
    fn get_table_color(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        for (key, (name, color)) in &self.inner.table_color {
            let value = PyDict::new(py);
            value.set_item("name", name)?;
            value.set_item("color", color)?;
            dict.set_item(*key as i32, value)?;
        }
        Ok(dict.into())
    }
}
