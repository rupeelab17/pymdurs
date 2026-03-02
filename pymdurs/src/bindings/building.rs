use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyList};
use rsmdu::geometric::building::BuildingCollection;

use crate::bindings::bounding_box::PyBoundingBox;
use crate::bindings::geo_core::PyGeoCore;

// Helper functions for converting Rust Vec to Python lists
fn vec_f64_to_pylist<'a>(py: Python<'a>, vec: &[f64]) -> PyResult<pyo3::Bound<'a, PyList>> {
    let list = PyList::empty(py);
    for v in vec {
        list.append(PyFloat::new(py, *v))?;
    }
    Ok(list)
}

fn vec_bool_to_pylist<'a>(py: Python<'a>, vec: &[bool]) -> PyResult<pyo3::Bound<'a, PyList>> {
    let list = PyList::empty(py);
    for v in vec {
        list.append(PyBool::new(py, *v))?;
    }
    Ok(list)
}

fn option_vec_f64_to_pylist<'a>(
    py: Python<'a>,
    vec: &[Option<f64>],
) -> PyResult<pyo3::Bound<'a, PyList>> {
    let list = PyList::empty(py);
    for v in vec {
        match v {
            Some(x) => list.append(PyFloat::new(py, *x))?,
            None => list.append(py.None())?,
        }
    }
    Ok(list)
}

/// BuildingCollection Python binding (exposed as Building to match Python API)
#[pyclass]
pub struct PyBuilding {
    inner: BuildingCollection,
}

#[pymethods]
impl PyBuilding {
    #[new]
    #[pyo3(signature = (filepath_shp = None, output_path = None, defaultStoreyHeight = 3.0, set_crs = None))]
    fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        #[allow(non_snake_case)] defaultStoreyHeight: f64,
        set_crs: Option<i32>,
    ) -> PyResult<Self> {
        match BuildingCollection::new(filepath_shp, output_path, defaultStoreyHeight, set_crs) {
            Ok(collection) => Ok(PyBuilding { inner: collection }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create BuildingCollection: {}",
                e
            ))),
        }
    }

    /// Set bounding box
    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> PyResult<()> {
        self.inner
            .set_bbox(min_x, min_y, max_x, max_y)
            .map_err(|e| PyValueError::new_err(format!("Failed to set bbox: {}", e)))
    }

    /// Run processing: load data, process heights, return self
    fn run(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
        // Use run_internal which works on &mut self
        slf.inner
            .run_internal()
            .map_err(|e| PyValueError::new_err(format!("Failed to run: {}", e)))?;
        Ok(slf)
    }

    /// Get number of buildings
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// Get GeoCore instance
    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }

    /// Convert to pandas DataFrame
    fn to_pandas(&self, py: Python) -> PyResult<Py<PyAny>> {
        // Convert Polars Rust DataFrame to pandas via Python polars package
        match self.inner.to_polars_df() {
            Ok(_polars_df_rust) => {
                let pandas = py.import("pandas")?;

                let mut height_vec: Vec<Option<f64>> = Vec::new();
                let mut area_vec: Vec<f64> = Vec::new();
                let mut centroid_x_vec: Vec<f64> = Vec::new();
                let mut centroid_y_vec: Vec<f64> = Vec::new();
                let mut nombre_d_etages_vec: Vec<Option<f64>> = Vec::new();
                let mut hauteur_2_vec: Vec<Option<f64>> = Vec::new();
                let mut no_hauteur_vec: Vec<bool> = Vec::new();

                for building in &self.inner.buildings {
                    height_vec.push(building.height);
                    area_vec.push(building.area);
                    centroid_x_vec.push(building.centroid.x());
                    centroid_y_vec.push(building.centroid.y());
                    nombre_d_etages_vec.push(building.nombre_d_etages);
                    hauteur_2_vec.push(building.hauteur_2);
                    no_hauteur_vec.push(building.no_hauteur);
                }

                // Convert to Python lists using helper functions
                let height_py = option_vec_f64_to_pylist(py, &height_vec)?;
                let area_py = vec_f64_to_pylist(py, &area_vec)?;
                let centroid_x_py = vec_f64_to_pylist(py, &centroid_x_vec)?;
                let centroid_y_py = vec_f64_to_pylist(py, &centroid_y_vec)?;
                let nombre_d_etages_py = option_vec_f64_to_pylist(py, &nombre_d_etages_vec)?;
                let hauteur_2_py = option_vec_f64_to_pylist(py, &hauteur_2_vec)?;
                let no_hauteur_py = vec_bool_to_pylist(py, &no_hauteur_vec)?;
                // Create pandas DataFrame
                let data = PyDict::new(py);
                data.set_item("hauteur", height_py)?;
                data.set_item("area", area_py)?;
                data.set_item("centroid_x", centroid_x_py)?;
                data.set_item("centroid_y", centroid_y_py)?;
                data.set_item("nombre_d_etages", nombre_d_etages_py)?;
                data.set_item("hauteur_2", hauteur_2_py)?;
                data.set_item("noHauteur", no_hauteur_py)?;

                let df: pyo3::Bound<PyAny> = pandas.call_method1("DataFrame", (data,))?;
                Ok(df.unbind())
            }
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to convert to DataFrame: {}",
                e
            ))),
        }
    }

    /// Load from GeoJSON
    #[staticmethod]
    fn from_geojson(
        geojson_data: &[u8],
        output_path: Option<String>,
        default_storey_height: f64,
        set_crs: Option<i32>,
    ) -> PyResult<Self> {
        match BuildingCollection::from_geojson(
            geojson_data,
            output_path,
            default_storey_height,
            set_crs,
        ) {
            Ok(collection) => Ok(PyBuilding { inner: collection }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to load from GeoJSON: {}",
                e
            ))),
        }
    }

    /// Load from IGN API
    #[staticmethod]
    fn from_ign_api(
        output_path: Option<String>,
        default_storey_height: f64,
        bbox: Option<PyBoundingBox>,
    ) -> PyResult<Self> {
        let bbox_rust = bbox.map(|pb| pb.inner);
        match BuildingCollection::from_ign_api(output_path, default_storey_height, bbox_rust) {
            Ok(collection) => Ok(PyBuilding { inner: collection }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to load from IGN API: {}",
                e
            ))),
        }
    }

    /// Get GeoJSON (equivalent to to_gdf() in Python)
    fn get_geojson(&self, py: Python) -> PyResult<Py<PyAny>> {
        match self.inner.get_geojson() {
            Ok(geojson) => {
                let json_str = geojson.to_string();
                let json = py.import("json")?;
                let geojson_dict: pyo3::Bound<PyAny> = json.call_method1("loads", (json_str,))?;
                Ok(geojson_dict.unbind())
            }
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to get GeoJSON: {}",
                e
            ))),
        }
    }
}
