use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use rsmdu::geometric::lidar::Lidar;
use std::path::Path;

use crate::bindings::geo_core::PyGeoCore;

/// Lidar Python binding.
/// Download and process LiDAR point clouds from IGN WFS; produces DSM, DTM, CHM GeoTIFFs.
/// LAZ files are cached under output_path/.cache/laz to avoid re-downloading.
#[pyclass]
pub struct PyLidar {
    inner: Lidar,
}

#[pymethods]
impl PyLidar {
    #[new]
    #[pyo3(signature = (output_path = None, classification = None, bbox = None))]
    #[pyo3(text_signature = "(output_path=None, classification=None, bbox=None)")]
    /// Create a Lidar instance.
    ///
    /// Args:
    ///     output_path: Directory for output GeoTIFFs and LAZ cache (default: temp).
    ///     classification: Optional default classification filter.
    ///     bbox: Optional (min_x, min_y, max_x, max_y) in WGS84; if set, fetches LAZ URLs immediately.
    fn new(
        output_path: Option<String>,
        classification: Option<u8>,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> PyResult<Self> {
        match Lidar::new(output_path, classification, bbox) {
            Ok(lidar) => Ok(PyLidar { inner: lidar }),
            Err(e) => Err(PyValueError::new_err(format!(
                "Failed to create Lidar: {}",
                e
            ))),
        }
    }

    /// Set bounding box (WGS84) and fetch LAZ URLs from IGN WFS, then load points.
    /// Call this before run() if bbox was not passed to the constructor.
    #[pyo3(text_signature = "(self, min_x, min_y, max_x, max_y)")]
    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> PyResult<()> {
        self.inner
            .set_bbox(min_x, min_y, max_x, max_y)
            .map_err(|e| {
                PyValueError::new_err(format!("Failed to set bbox and get LiDAR points: {}", e))
            })
    }

    /// Set classification filter (optional).
    fn set_classification(&mut self, classification: Option<u8>) {
        self.inner.set_classification(classification);
    }

    /// Set CRS (e.g. 2154 for Lambert-93). Default is 2154.
    fn set_crs(&mut self, epsg: i32) {
        self.inner.geo_core.set_epsg(epsg);
    }

    /// Run LiDAR processing: build DSM/DTM/CHM from loaded points and save GeoTIFF.
    ///
    /// Args:
    ///     file_name: Output filename (e.g. "DSM.tif", "CDSM.tif"). Default "lidar_cdsm.tif".
    ///     classification_list: Point classes to include (e.g. [2,6,9] for ground+buildings, [3,4,5] for vegetation).
    ///     resolution: Pixel size in metres. Default 1.0.
    ///     write_out_file: If True, write GeoTIFF to disk.
    ///
    /// Returns:
    ///     Path to the created GeoTIFF (3 bands: DSM, DTM, CHM).
    #[pyo3(signature = (file_name = None, classification_list = None, resolution = None, write_out_file = true))]
    #[pyo3(
        text_signature = "(self, file_name=None, classification_list=None, resolution=None, write_out_file=True)"
    )]
    fn run(
        mut slf: PyRefMut<Self>,
        file_name: Option<String>,
        classification_list: Option<Vec<u8>>,
        resolution: Option<f64>,
        write_out_file: bool,
    ) -> PyResult<String> {
        (*slf)
            .inner
            .run(file_name, classification_list, resolution, write_out_file)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|e| PyValueError::new_err(format!("Failed to run Lidar: {}", e)))
    }

    /// Return the output directory path (string).
    fn get_output_path(&self) -> String {
        self.inner.get_output_path().to_string_lossy().to_string()
    }

    /// Export loaded LiDAR points (ROI of the current BBOX) to a LAS file.
    /// Uses points already loaded by set_bbox(). Use .las for uncompressed, .laz for compressed.
    ///
    /// Args:
    ///     filename: Output path (e.g. "bbox.las"). If relative, saved under the Lidar output directory.
    ///
    /// Returns:
    ///     Absolute path to the written file.
    #[pyo3(signature = (filename = "bbox.las"))]
    #[pyo3(text_signature = "(self, filename='bbox.las')")]
    fn save(&self, filename: &str) -> PyResult<String> {
        self.inner
            .save_las(Path::new(filename))
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| PyValueError::new_err(format!("Failed to save LAS: {}", e)))
    }

    /// GeoCore instance (bbox, epsg, output_path).
    #[getter]
    fn geo_core(&self) -> PyGeoCore {
        PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
