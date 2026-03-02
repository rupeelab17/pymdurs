//! Vegetation zones for URock wind field (sect. 2.3.3, Appendix B8–B10).
//!
//! Vegetation in built-up area (B8) vs open area (B9–B10); factors applied as multiplier to building wind factor.

use geo::algorithm::Contains;
use geo::{Point, Polygon};

/// Single vegetation patch: footprint and canopy height for wind attenuation (Nelson et al. 2009).
#[derive(Debug, Clone)]
pub struct VegetationPatch {
    /// Footprint polygon (project CRS).
    pub footprint: Polygon<f64>,
    /// Canopy top height [m] above ground.
    pub canopy_height_m: f64,
    /// Roughness length [m] for log profile (default 0.1).
    pub z0: f64,
    /// Attenuation coefficient αi (default 2.0).
    pub alpha_i: f64,
}

impl VegetationPatch {
    pub fn new(footprint: Polygon<f64>, canopy_height_m: f64) -> Self {
        Self {
            footprint,
            canopy_height_m: canopy_height_m.max(0.01),
            z0: 0.1,
            alpha_i: 2.0,
        }
    }

    pub fn with_z0(mut self, z0: f64) -> Self {
        self.z0 = z0.max(0.01);
        self
    }

    pub fn with_alpha_i(mut self, alpha_i: f64) -> Self {
        self.alpha_i = alpha_i;
        self
    }

    /// True if (geo_x, geo_y) is inside this patch.
    pub fn contains_point(&self, geo_x: f64, geo_y: f64) -> bool {
        let pt = Point::new(geo_x, geo_y);
        self.footprint.contains(&pt)
    }
}

/// Collection of vegetation patches for wind field (URock Task 4: multiply building factor by vegetation factor).
#[derive(Debug, Clone, Default)]
pub struct VegetationCollection {
    pub patches: Vec<VegetationPatch>,
}

impl VegetationCollection {
    pub fn new() -> Self {
        Self { patches: Vec::new() }
    }

    pub fn add(&mut self, patch: VegetationPatch) {
        self.patches.push(patch);
    }

    /// First patch containing (geo_x, geo_y), if any.
    pub fn patch_at(&self, geo_x: f64, geo_y: f64) -> Option<&VegetationPatch> {
        self.patches
            .iter()
            .find(|p| p.contains_point(geo_x, geo_y))
    }
}

// ---------- URock B8, B9, B10 ----------

/// Vegetation in built-up area (B8): V0/Vp(z) = [ln(Hvtm/z0)/ln(z/z0)] · exp(αi·(z/Hvtm − 1)).
pub fn vegetation_factor_built(z: f64, h_vtm: f64, z0: f64, alpha_i: f64) -> f64 {
    if h_vtm <= 0.0 || z <= 0.0 {
        return 1.0;
    }
    let z = z.max(0.01);
    let z0 = z0.max(0.01);
    if z >= h_vtm {
        return 1.0;
    }
    let ln_ratio = (h_vtm / z0).ln() / (z / z0).ln();
    let exp_term = (alpha_i * (z / h_vtm - 1.0)).exp();
    (ln_ratio * exp_term).max(0.0).min(1.0)
}

/// Vegetation in open area, below canopy (B9): V0/Vp(z) = ln((Hvtm−d)/z0)/ln(z/z0) · exp(αi·(z/Hvtm − 1)).
pub fn vegetation_factor_open_below(z: f64, h_vtm: f64, z0: f64, d: f64, alpha_i: f64) -> f64 {
    if h_vtm <= 0.0 || z <= 0.0 {
        return 1.0;
    }
    let z = z.max(0.01);
    let z0 = z0.max(0.01);
    if z >= h_vtm {
        return 1.0;
    }
    let h_minus_d = (h_vtm - d).max(z0 + 0.01);
    let ln_ratio = (h_minus_d / z0).ln() / (z / z0).ln();
    let exp_term = (alpha_i * (z / h_vtm - 1.0)).exp();
    (ln_ratio * exp_term).max(0.0).min(1.0)
}

/// Vegetation in open area, above canopy (B10): V0/Vp(z) = ln((z−d)/z0)/ln(z/z0).
pub fn vegetation_factor_open_above(z: f64, z0: f64, d: f64) -> f64 {
    if z <= 0.0 {
        return 1.0;
    }
    let z = z.max(0.01);
    let z0 = z0.max(0.01);
    let z_minus_d = (z - d).max(z0 + 0.01);
    ((z_minus_d / z0).ln() / (z / z0).ln()).max(0.0).min(1.0)
}

/// Combined vegetation factor: built-up (B8) or open (B9 below canopy, B10 above).
pub fn vegetation_wind_factor(
    z: f64,
    h_vtm: f64,
    z0: f64,
    d: f64,
    alpha_i: f64,
    in_built_up: bool,
) -> f64 {
    if in_built_up {
        vegetation_factor_built(z, h_vtm, z0, alpha_i)
    } else if z < h_vtm {
        vegetation_factor_open_below(z, h_vtm, z0, d, alpha_i)
    } else {
        vegetation_factor_open_above(z, z0, d)
    }
}
