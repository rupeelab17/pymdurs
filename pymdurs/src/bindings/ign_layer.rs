//! Shared PyO3 bindings for IGN WFS layers (Cadastre / Road / Iris).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use rsmdu::geometric::cadastre::Cadastre;
use rsmdu::geometric::iris::Iris;
use rsmdu::geometric::road::Road;

use crate::bindings::geo_core::PyGeoCore;

/// Emit a thin `#[pyclass]` over an IgnLayer alias (`Cadastre` / `Road` / `Iris`).
macro_rules! ign_pyclass {
    ($py_name:ident, $rust_ty:ty, $label:literal) => {
        #[gen_stub_pyclass(module = "pymdurs.geometric")]
        #[pyclass]
        pub struct $py_name {
            inner: $rust_ty,
        }

        #[gen_stub_pymethods]
        #[pymethods]
        impl $py_name {
            #[new]
            #[pyo3(signature = (output_path = None))]
            fn new(output_path: Option<String>) -> PyResult<Self> {
                <$rust_ty>::new(output_path)
                    .map(|inner| Self { inner })
                    .map_err(|e| {
                        PyValueError::new_err(format!("Failed to create {}: {}", $label, e))
                    })
            }

            /// Set bounding box
            fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
                self.inner.set_bbox(min_x, min_y, max_x, max_y);
            }

            /// Set CRS
            fn set_crs(&mut self, epsg: i32) {
                self.inner.set_crs(epsg);
            }

            /// Run IGN download + GeoJSON parse
            #[gen_stub(override_return_type(type_repr = "Self"))]
            fn run(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
                let py = slf.py();
                let this = &mut *slf;
                py.detach(|| this.inner.run_internal())
                    .map_err(|e| PyValueError::new_err(format!("Failed to run {}: {e}", $label)))?;
                Ok(slf)
            }

            /// Get GeoJSON (equivalent to to_gdf() in Python)
            #[gen_stub(override_return_type(
                        type_repr = "dict[str, typing.Any]",
                        imports = ("typing",)
                    ))]
            fn get_geojson(&self, py: Python) -> PyResult<Py<PyAny>> {
                match self.inner.get_geojson() {
                    Some(geojson) => {
                        let json_str = geojson.to_string();
                        let json = py.import("json")?;
                        let geojson_dict: Bound<PyAny> = json.call_method1("loads", (json_str,))?;
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
                    .map_err(|e| PyValueError::new_err(format!("Failed to save GeoJSON: {e}")))
            }

            /// Get output path
            fn get_output_path(&self) -> String {
                self.inner.get_output_path().to_string_lossy().into_owned()
            }

            /// Get GeoCore instance
            #[getter]
            fn geo_core(&self) -> PyGeoCore {
                PyGeoCore {
                    inner: self.inner.clone_geo_core(),
                }
            }
        }
    };
}

ign_pyclass!(PyCadastre, Cadastre, "Cadastre");
ign_pyclass!(PyRoad, Road, "Road");
ign_pyclass!(PyIris, Iris, "Iris");
