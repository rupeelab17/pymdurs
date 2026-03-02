//! Python bindings for rsmdu::thermal::WindField (Röckle urban wind).
//! URock 2023a (Bernard et al., GMD 16, 5703–5727).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::path::Path;

use rsmdu::thermal::{VegetationCollection, WindConfig, WindField, WindProfileType};

/// WindConfig: reference speed, direction, height, resolution, profile type, optional roughness.
///
/// Grid spacing for the 3D solver: solver_dx/solver_dy override raster pixel size; solver_dz overrides
/// the vertical step. When solver_dz is None, resolution_m is used as the vertical spacing (dz).
#[pyclass]
pub struct PyWindConfig {
    #[pyo3(get, set)]
    pub wind_speed_ref: f64,
    #[pyo3(get, set)]
    pub wind_direction: f64,
    #[pyo3(get, set)]
    pub z_ref: f64,
    /// Nominal resolution [m]. Also the default vertical step (dz) for the solver when solver_dz is None.
    #[pyo3(get, set)]
    pub resolution_m: f64,
    /// Vertical profile: "log", "power_law" (default), "urban".
    #[pyo3(get, set)]
    pub profile_type: String,
    #[pyo3(get, set)]
    pub z0: Option<f64>,
    #[pyo3(get, set)]
    pub d: Option<f64>,
    #[pyo3(get, set)]
    pub hr: Option<f64>,
    #[pyo3(get, set)]
    pub lambda_f: Option<f64>,
    #[pyo3(get, set)]
    pub use_mass_consistent_solver: bool,
    #[pyo3(get, set)]
    pub solver_epsilon: f64,
    #[pyo3(get, set)]
    pub solver_max_iter: u32,
    #[pyo3(get, set)]
    pub output_height: f64,
    /// Solver grid spacing [m]: x. If None, use raster pixel size.
    #[pyo3(get, set)]
    pub solver_dx: Option<f64>,
    /// Solver grid spacing [m]: y. If None, use raster pixel size.
    #[pyo3(get, set)]
    pub solver_dy: Option<f64>,
    /// Solver grid spacing [m]: z (vertical). If None, use resolution_m.
    #[pyo3(get, set)]
    pub solver_dz: Option<f64>,
    /// If true, write rockle_zone.tif with Röckle zone codes (0=Freestream, 1=Inside, 2=Upwind, 3=UpwindVortex, 4=Cavity, 5=Wake, etc.).
    #[pyo3(get, set)]
    pub save_rockle_zone: bool,
}

fn parse_profile_type(s: &str) -> WindProfileType {
    match s.to_lowercase().as_str() {
        "log" => WindProfileType::Log,
        "urban" => WindProfileType::Urban,
        _ => WindProfileType::PowerLaw,
    }
}

#[pymethods]
impl PyWindConfig {
    #[new]
    #[pyo3(signature = (
        wind_speed_ref=3.5,
        wind_direction=225.0,
        z_ref=10.0,
        resolution_m=2.0,
        profile_type="power_law",
        z0=None,
        d=None,
        hr=None,
        lambda_f=None,
        use_mass_consistent_solver=false,
        solver_epsilon=0.0001,
        solver_max_iter=5000,
        output_height=2.0,
        solver_dx=None,
        solver_dy=None,
        solver_dz=None,
        save_rockle_zone=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        wind_speed_ref: f64,
        wind_direction: f64,
        z_ref: f64,
        resolution_m: f64,
        profile_type: &str,
        z0: Option<f64>,
        d: Option<f64>,
        hr: Option<f64>,
        lambda_f: Option<f64>,
        use_mass_consistent_solver: bool,
        solver_epsilon: f64,
        solver_max_iter: u32,
        output_height: f64,
        solver_dx: Option<f64>,
        solver_dy: Option<f64>,
        solver_dz: Option<f64>,
        save_rockle_zone: bool,
    ) -> Self {
        PyWindConfig {
            wind_speed_ref,
            wind_direction,
            z_ref,
            resolution_m,
            profile_type: profile_type.to_string(),
            z0,
            d,
            hr,
            lambda_f,
            use_mass_consistent_solver,
            solver_epsilon,
            solver_max_iter,
            output_height,
            solver_dx,
            solver_dy,
            solver_dz,
            save_rockle_zone,
        }
    }
}

/// WindField: Röckle-based urban wind solver (outputs wind_speed.tif, wind_direction.tif; optionally rockle_zone.tif when save_rockle_zone=True).
#[pyclass]
pub struct PyWindField {
    inner: WindField,
}

#[pymethods]
impl PyWindField {
    #[new]
    #[pyo3(signature = (output_path = None))]
    fn new(output_path: Option<String>) -> PyResult<Self> {
        WindField::new(output_path)
            .map(|w| PyWindField { inner: w })
            .map_err(|e| PyValueError::new_err(format!("Failed to create WindField: {}", e)))
    }

    /// Set bounding box (min_x, min_y, max_x, max_y) in project CRS.
    fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.inner.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Run wind field: requires DEM and DSM GeoTIFF paths and a BuildingCollection.
    /// Writes wind_speed.tif and wind_direction.tif to the output path; if save_rockle_zone is True, also writes rockle_zone.tif.
    /// Returns (wind_speed_path, wind_direction_path, rockle_zone_path). rockle_zone_path is None when save_rockle_zone is False.
    fn run(
        &self,
        config: &PyWindConfig,
        dem_path: &str,
        dsm_path: &str,
        buildings: &crate::bindings::building::PyBuilding,
    ) -> PyResult<(String, String, Option<String>)> {
        let mut cfg = WindConfig::default();
        cfg.wind_speed_ref = config.wind_speed_ref;
        cfg.wind_direction = config.wind_direction;
        cfg.z_ref = config.z_ref;
        cfg.resolution_m = config.resolution_m;
        cfg.profile_type = parse_profile_type(&config.profile_type);
        cfg.z0 = config.z0;
        cfg.d = config.d;
        cfg.hr = config.hr;
        cfg.lambda_f = config.lambda_f;
        cfg.use_mass_consistent_solver = config.use_mass_consistent_solver;
        cfg.solver_epsilon = config.solver_epsilon;
        cfg.solver_max_iter = config.solver_max_iter as usize;
        cfg.output_height = config.output_height;
        cfg.solver_dx = config.solver_dx;
        cfg.solver_dy = config.solver_dy;
        cfg.solver_dz = config.solver_dz;
        cfg.save_rockle_zone = config.save_rockle_zone;
        let result = self
            .inner
            .run(
                cfg,
                Path::new(dem_path),
                Path::new(dsm_path),
                &buildings.inner,
                None::<&VegetationCollection>,
            )
            .map_err(|e| PyValueError::new_err(format!("WindField run failed: {}", e)))?;
        let rockle_zone_path = result
            .rockle_zone_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        Ok((
            result.wind_speed_path.to_string_lossy().to_string(),
            result.wind_direction_path.to_string_lossy().to_string(),
            rockle_zone_path,
        ))
    }

    /// Get GeoCore instance
    #[getter]
    fn geo_core(&self) -> crate::bindings::geo_core::PyGeoCore {
        crate::bindings::geo_core::PyGeoCore {
            inner: self.inner.geo_core.clone(),
        }
    }
}
