//! Bindings PyO3 pour MNT / MNS / MNH (même logique, kind distinct).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;
use rsmdu::geometric::lidar_raster::{LidarRaster, MnhKind, MnsKind, MntKind};

use crate::bindings::geo_core::PyGeoCore;

macro_rules! py_lidar_raster {
    ($py_ty:ident, $kind:ty, $create_label:literal, $run_label:literal) => {
        #[gen_stub_pyclass(module = "pymdurs.geometric")]
        #[pyclass]
        pub struct $py_ty {
            inner: LidarRaster<$kind>,
        }

        #[gen_stub_pymethods]
        #[pymethods]
        impl $py_ty {
            #[new]
            #[pyo3(signature = (output_path = None))]
            fn new(output_path: Option<String>) -> PyResult<Self> {
                LidarRaster::<$kind>::new(output_path)
                    .map(|inner| Self { inner })
                    .map_err(|e| {
                        PyValueError::new_err(format!(
                            "Failed to create {}: {}",
                            $create_label, e
                        ))
                    })
            }

            fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
                self.inner.set_bbox(min_x, min_y, max_x, max_y);
            }

            fn set_crs(&mut self, epsg: i32) {
                self.inner.set_crs(epsg);
            }

            #[gen_stub(override_return_type(type_repr = "Self"))]
            fn run(mut slf: PyRefMut<Self>) -> PyResult<PyRefMut<Self>> {
                slf.inner.run_internal().map_err(|e| {
                    PyValueError::new_err(format!("Failed to run {}: {}", $run_label, e))
                })?;
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
    };
}

py_lidar_raster!(PyMnt, MntKind, "Mnt", "MNT");
py_lidar_raster!(PyMns, MnsKind, "Mns", "MNS");
py_lidar_raster!(PyMnh, MnhKind, "Mnh", "MNH");
