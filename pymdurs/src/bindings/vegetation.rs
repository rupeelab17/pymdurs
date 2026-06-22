use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use rsmdu::geometric::vegetation::Vegetation;

use crate::bindings::geo_core::PyGeoCore;

/// Vegetation Python binding
#[gen_stub_pyclass(module = "pymdurs.geometric")]
#[pyclass]
pub struct PyVegetation {
    inner: Vegetation,
}

#[gen_stub_pymethods]
#[pymethods]
impl PyVegetation {
    #[new]
    #[pyo3(signature = (filepath_shp = None, output_path = None, set_crs = None, write_file = false, min_area = 0.0))]
    fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
        write_file: bool,
        min_area: f64,
    ) -> PyResult<Self> {
        match Vegetation::new(filepath_shp, output_path, set_crs, write_file, min_area) {
            Ok(vegetation) => Ok(PyVegetation { inner: vegetation }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Vegetation: {}",
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

    /// Run vegetation processing: calculate NDVI from IRC or load from shapefile
    #[gen_stub(override_return_type(type_repr = "Self"))]
    fn run(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
        // Use run_internal which works on &mut self
        slf.inner
            .run_internal()
            .map_err(|e| PyValueError::new_err(format!("Failed to run Vegetation: {}", e)))?;
        Ok(slf)
    }

    /// Get GeoJSON (equivalent to to_gdf() in Python)
    #[gen_stub(override_return_type(type_repr = "dict[str, typing.Any]", imports = ("typing",)))]
    fn get_geojson(&self, py: Python) -> PyResult<Py<PyAny>> {
        match self.inner.get_geojson() {
            Some(geojson) => {
                // Convert GeoJSON to Python dict using serde_json
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

    /// Save to GeoJSON file
    #[pyo3(signature = (name = None))]
    fn to_geojson(&self, name: Option<&str>) -> PyResult<()> {
        self.inner
            .to_geojson(name)
            .map_err(|e| PyValueError::new_err(format!("Failed to save GeoJSON: {}", e)))
    }

    /// Get output path
    fn get_output_path(&self) -> String {
        self.inner.get_output_path().to_string_lossy().to_string()
    }

    /// Get GeoCore instance
    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }

    /// Get minimum area filter
    #[getter]
    fn min_area(&self) -> f64 {
        self.inner.get_min_area()
    }
}
