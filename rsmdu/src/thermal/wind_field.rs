//! Röckle-based urban wind field: analytical zones and parallel raster solver.
//!
//! Aligned with URock 2023a (Bernard et al., GMD 16, 5703–5727):
//! zone formulas (Appendix A), wind factors (Appendix B), profiles (sect. 2.2), Weff/Leff (Eqs 4–5).
//! References: Röckle (1990), Kaplan & Dinar (1996), Bagal et al. (2004), Pol et al. (2006), Nelson et al. (2008).
//!
//! Includes: mass-consistent solver (Eqs 7–9), vegetation zones (B8–B10). Street canyon: still simplified.

use anyhow::{Context, Result};
use ndarray::{Array2, Array3};
use std::io::Write;
use std::path::Path;

use super::mass_consistent_solver::solve_mass_consistent;
use super::vegetation::{vegetation_wind_factor, VegetationCollection};

use crate::geo_core::{BoundingBox, GeoCore};
use crate::geometric::building::{Building, BuildingCollection};

#[cfg(feature = "gdal")]
use gdal::raster::Buffer;
#[cfg(feature = "gdal")]
use gdal::spatial_ref::SpatialRef;
#[cfg(feature = "gdal")]
use gdal::{Dataset, Metadata};
#[cfg(feature = "rayon")]
use rayon::prelude::*;

// ---------- WindTransform ----------

/// Converts between geographic coordinates and wind-aligned frame (along-wind, cross-wind).
/// Meteorological convention: 0° = wind from North, clockwise.
pub struct WindTransform {
    angle_rad: f64,
}

impl WindTransform {
    /// * `wind_direction_deg`: direction the wind is coming FROM (meteo), in degrees.
    pub fn new(wind_direction_deg: f64) -> Self {
        let angle_rad = (270.0 - wind_direction_deg).to_radians();
        Self { angle_rad }
    }

    /// (x, y) in project CRS → (along_wind, cross_wind).
    pub fn to_wind_frame(&self, x: f64, y: f64) -> (f64, f64) {
        let cos_a = self.angle_rad.cos();
        let sin_a = self.angle_rad.sin();
        let along = x * cos_a + y * sin_a;
        let cross = -x * sin_a + y * cos_a;
        (along, cross)
    }

    /// (along_wind, cross_wind) → (x, y) in project CRS.
    pub fn from_wind_frame(&self, along: f64, cross: f64) -> (f64, f64) {
        let cos_a = self.angle_rad.cos();
        let sin_a = self.angle_rad.sin();
        let x = along * cos_a - cross * sin_a;
        let y = along * sin_a + cross * cos_a;
        (x, y)
    }
}

// ---------- WindZone ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindZone {
    Inside,
    /// Displacement zone (quarter ellipse upwind); Appendix A1.
    Upwind,
    /// Displacement vortex (smaller quarter ellipse when wind ~ perpendicular to facade); Appendix A2.
    UpwindVortex,
    Cavity,
    Wake,
    /// Rooftop perpendicular (half elliptical cylinder above roof); Appendix A4–A5.
    RooftopPerp,
    /// Rooftop corner (pyramid above roof); Appendix A5.
    RooftopCorner,
    /// Street canyon between two buildings; Appendix B7.
    StreetCanyon,
    Freestream,
}

// ---------- RockleBuilding ----------

/// One building in wind-aligned frame: centre (cx, cy), height h, bbox width w (cross-wind), length l (along-wind),
/// and effective dimensions weff, leff (URock Eqs 4–5: Weff = WBBox·(AB/ABBox), Leff = LBBox·(AB/ABBox)).
pub struct RockleBuilding {
    pub cx: f64,
    pub cy: f64,
    pub h: f64,
    /// Bounding box width (cross-wind).
    pub w: f64,
    /// Bounding box length (along-wind).
    pub l: f64,
    /// Effective width for zone formulas (Eq 4).
    pub weff: f64,
    /// Effective length for zone formulas (Eq 5).
    pub leff: f64,
}

impl RockleBuilding {
    /// Upwind (displacement) zone length. URock Eq A1: Lf = 1.5·Weff/(1+0.8·Weff/H).
    pub fn upwind_length(&self) -> f64 {
        if self.h <= 0.0 || self.weff <= 0.0 {
            return 0.0;
        }
        1.5 * self.weff / (1.0 + 0.8 * self.weff / self.h)
    }

    /// Cavity (recirculation) length. URock Eq A4: Lr = 1.8·Weff/((Leff/H)^0.3·(1+0.24·Leff/H)) (Kaplan & Dinar 1996).
    pub fn cavity_length(&self) -> f64 {
        if self.h <= 0.0 || self.weff <= 0.0 {
            return 0.0;
        }
        let leff_over_h = (self.leff / self.h).max(1e-6);
        1.8 * self.weff / (leff_over_h.powf(0.3) * (1.0 + 0.24 * self.leff / self.h))
    }

    /// Wake length. URock: Lw = 3·Lr (Kaplan & Dinar 1996).
    pub fn wake_length(&self) -> f64 {
        3.0 * self.cavity_length()
    }

    /// Characteristic height for wake half-width expansion. URock Eq A5: Hcm = 0.22·(0.67·min(HF,Weff)+0.33·max(HF,Weff)) (Pol et al. 2006).
    pub fn displacement_height(&self) -> f64 {
        if self.h <= 0.0 || self.weff <= 0.0 {
            return 0.0;
        }
        let (mn, mx) = (self.h.min(self.weff), self.h.max(self.weff));
        0.22 * (0.67 * mn + 0.33 * mx)
    }

    /// Displacement vortex zone length. URock Eq A2: Lfv = 0.6·Weff/(1+0.8·Weff/HF) (Bagal et al. 2004).
    pub fn upwind_vortex_length(&self) -> f64 {
        if self.h <= 0.0 || self.weff <= 0.0 {
            return 0.0;
        }
        0.6 * self.weff / (1.0 + 0.8 * self.weff / self.h)
    }

    /// Classify point (relative to building centre, in wind frame) into a zone.
    /// Uses ellipse geometry for displacement (A1), displacement vortex (A2), cavity (A3), wake (sect. 2.3.2).
    pub fn classify_point(&self, rel_along: f64, rel_cross: f64) -> WindZone {
        let half_l = self.l / 2.0;
        let half_w = self.w / 2.0;
        let lu = self.upwind_length();
        let lfv = self.upwind_vortex_length();
        let lr = self.cavity_length();
        let lw = self.wake_length();

        if rel_along.abs() <= half_l && rel_cross.abs() <= half_w {
            return WindZone::Inside;
        }

        // Upwind: quarter ellipse (A1). dy = distance from upwind facade; (dy/Lf)² + (rel_cross/half_w)² <= 1.
        let dy_up = -(rel_along + half_l);
        if dy_up > 0.0 && dy_up <= lu && half_w > 1e-10 {
            let in_ellipse = (dy_up / lu).powi(2) + (rel_cross / half_w).powi(2) <= 1.0;
            if in_ellipse {
                // Displacement vortex (A2): smaller ellipse when wind ~ perpendicular; test vortex first.
                if lfv > 1e-10
                    && dy_up <= lfv
                    && (dy_up / lfv).powi(2) + (rel_cross / half_w).powi(2) <= 1.0
                {
                    return WindZone::UpwindVortex;
                }
                return WindZone::Upwind;
            }
        }

        // Cavity (A3): ellipse (dy/Lr)² + (rel_cross/half_w)² <= 1, dy in (0, Lr].
        let dy_down = rel_along - half_l;
        if dy_down > 0.0 && dy_down <= lr && half_w > 1e-10 {
            let half_width_cavity = half_w * (1.0 - (dy_down / lr).powi(2)).max(0.0).sqrt();
            if rel_cross.abs() <= half_width_cavity {
                return WindZone::Cavity;
            }
        }

        // Wake: same ellipse shape, 3× length (dy in (Lr, Lw]).
        if dy_down > lr && dy_down <= lw && half_w > 1e-10 {
            let half_width_wake = half_w * (1.0 - (dy_down / lw).powi(2)).max(0.0).sqrt();
            if rel_cross.abs() <= half_width_wake {
                return WindZone::Wake;
            }
        }

        WindZone::Freestream
    }
}

/// Build RockleBuilding list from BuildingCollection and wind direction.
/// Effective dimensions weff, leff follow URock Eqs 4–5: Weff = WBBox·(AB/ABBox), Leff = LBBox·(AB/ABBox).
pub fn buildings_to_rockle(
    buildings: &[Building],
    wind_tf: &WindTransform,
    default_storey_height: f64,
) -> Vec<RockleBuilding> {
    let mut out = Vec::with_capacity(buildings.len());
    for b in buildings {
        let h = b.get_height(default_storey_height);
        let c = b.centroid;
        let cx = c.x();
        let cy = c.y();
        let (along_center, cross_center) = wind_tf.to_wind_frame(cx, cy);

        let mut min_along = along_center;
        let mut max_along = along_center;
        let mut min_cross = cross_center;
        let mut max_cross = cross_center;
        for coord in b.footprint.exterior().coords() {
            let (a, cr) = wind_tf.to_wind_frame(coord.x, coord.y);
            min_along = min_along.min(a);
            max_along = max_along.max(a);
            min_cross = min_cross.min(cr);
            max_cross = max_cross.max(cr);
        }
        let l = (max_along - min_along).max(0.1);
        let w = (max_cross - min_cross).max(0.1);
        let area_bbox = l * w;
        let area_footprint = b.area.max(1e-10);
        let ratio = (area_footprint / area_bbox).min(1.0);
        let weff = w * ratio;
        let leff = l * ratio;
        out.push(RockleBuilding {
            cx,
            cy,
            h,
            w,
            l,
            weff,
            leff,
        });
    }
    out
}

// ---------- Wind profile and roughness ----------

const Z0_URBAN_DEFAULT: f64 = 0.5;

/// Vertical wind profile type (URock sect. 2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindProfileType {
    /// Logarithmic: U(z)/U(z_ref) = ln(z/z0) / ln(z_ref/z0).
    Log,
    /// Power law Eq 2: U(z) = Vref·(z/zref)^p, p = 0.12·z0 + 0.18 (Matzarakis & Endler 2009).
    #[default]
    PowerLaw,
    /// Urban Eq 3: exponential below Hr, logarithmic above (Cionco 1972, Macdonald 2000).
    Urban,
}

/// Hanna & Britter (2002) Table 1: displacement d and roughness z0 from λf and Hr.
/// All d values scale with Hr (geometric mean obstacle height).
pub fn roughness_hanna_britter(lambda_f: f64, hr: f64) -> (f64, f64) {
    let lambda_f = lambda_f.min(1.0);
    let (d, z0) = if lambda_f <= 0.05 {
        (3.0 * lambda_f * hr, lambda_f * hr)
    } else if lambda_f < 0.15 {
        ((0.15 + 5.5 * (lambda_f - 0.05)) * hr, lambda_f * hr)
    } else if lambda_f < 1.0 {
        ((0.7 + 0.35 * (lambda_f - 0.15)) * hr, 0.15 * hr)
    } else {
        (hr, 0.15 * hr)
    };
    (d.max(0.0), z0.max(0.01))
}

/// Compute roughness and morphometry from buildings in wind-aligned frame. Returns (z0, d, hr, lambda_f).
/// lambda_f = Af/AT (frontal area / domain area), Hr = area-weighted geometric mean height.
pub fn compute_roughness_from_buildings(
    buildings: &[RockleBuilding],
    domain_area: f64,
) -> (f64, f64, f64, f64) {
    if domain_area <= 0.0 || buildings.is_empty() {
        return (Z0_URBAN_DEFAULT, 0.0, 10.0, 0.1);
    }
    let af: f64 = buildings.iter().map(|b| b.h * b.w).sum();
    let lambda_f = (af / domain_area).min(2.0);
    let total_area: f64 = buildings.iter().map(|b| b.w * b.l).sum();
    if total_area <= 0.0 {
        return (Z0_URBAN_DEFAULT, 0.0, 10.0, lambda_f);
    }
    let hr = buildings
        .iter()
        .map(|b| {
            let area = b.w * b.l;
            area * (b.h.max(0.1)).ln()
        })
        .sum::<f64>()
        / total_area;
    let hr = hr.exp();
    let (d, z0) = roughness_hanna_britter(lambda_f, hr);
    (z0, d, hr, lambda_f)
}

/// Logarithmic wind profile: U(z)/U(z_ref) = ln(z/z0) / ln(z_ref/z0).
pub fn log_wind_profile(z: f64, z_ref: f64) -> f64 {
    log_wind_profile_with_z0(z, z_ref, Z0_URBAN_DEFAULT)
}

/// Logarithmic profile with given z0.
pub fn log_wind_profile_with_z0(z: f64, z_ref: f64, z0: f64) -> f64 {
    let z = z.max(0.01);
    let z_ref = z_ref.max(0.01);
    let z0 = z0.max(0.01);
    (z / z0).ln() / (z_ref / z0).ln()
}

/// Power law profile (URock Eq 2): U(z)/U(z_ref) = (z/z_ref)^p, p = 0.12·z0 + 0.18.
pub fn power_law_profile(z: f64, z_ref: f64, z0: f64) -> f64 {
    let z = z.max(0.01);
    let z_ref = z_ref.max(0.01);
    let p = 0.12 * z0 + 0.18;
    (z / z_ref).powf(p)
}

/// Urban profile (URock Eq 3): exponential below Hr, logarithmic above.
pub fn urban_profile(z: f64, z_ref: f64, z0: f64, d: f64, hr: f64, lambda_f: f64) -> f64 {
    let z = z.max(0.01);
    let z_ref = z_ref.max(0.01);
    let a = 9.6 * lambda_f;
    if z < hr {
        (a * (z / hr - 1.0)).exp()
    } else {
        let z0 = z0.max(0.01);
        ((z - d) / z0).ln() / ((z_ref - d) / z0).ln()
    }
}

/// Wind profile ratio U(z)/U(z_ref) depending on profile type.
pub fn wind_profile_ratio(
    z: f64,
    z_ref: f64,
    profile_type: WindProfileType,
    z0: f64,
    d: f64,
    hr: f64,
    lambda_f: f64,
) -> f64 {
    match profile_type {
        WindProfileType::Log => log_wind_profile_with_z0(z, z_ref, z0),
        WindProfileType::PowerLaw => power_law_profile(z, z_ref, z0),
        WindProfileType::Urban => urban_profile(z, z_ref, z0, d, hr, lambda_f),
    }
}

// ---------- Zone speed factor (uses profile) ----------

// URock Appendix B constants (Bagal et al. 2004, Kaplan & Dinar 1996).
const CDZ: f64 = 0.4;
const P_PROFILE: f64 = 0.16;

/// Wind factor and reference height for initial speed: speed = factor * Vwp(z_vwp).
/// Uses profile_ratio for U(z)/U(z_ref). Returns (factor, z_vwp).
fn zone_wind_factor_and_ref(
    zone: WindZone,
    rel_along: f64,
    _rel_cross: f64,
    building: &RockleBuilding,
    z_pixel: f64,
    z_ref: f64,
    profile_type: WindProfileType,
    z0: f64,
    d: f64,
    hr: f64,
    lambda_f: f64,
) -> (f64, f64) {
    let profile = |z: f64, zr: f64| wind_profile_ratio(z, zr, profile_type, z0, d, hr, lambda_f);
    let half_l = building.l / 2.0;
    let h = building.h;
    match zone {
        WindZone::Freestream
        | WindZone::RooftopPerp
        | WindZone::RooftopCorner
        | WindZone::StreetCanyon => {
            let factor = if z_pixel > h {
                profile(z_pixel, z_ref)
            } else {
                1.0
            };
            (factor, z_pixel)
        }
        WindZone::UpwindVortex => {
            let dy = -(rel_along + half_l);
            let dodv = building.upwind_vortex_length();
            if dodv <= 0.0 || h <= 0.0 {
                return (CDZ * 0.5 * profile(h, z_ref), z_pixel);
            }
            let hdv = 0.5 * h * (1.0 - (dy / dodv).powi(2)).max(0.0).sqrt();
            let wf = if z_pixel < hdv {
                let v_ratio = -(0.6 * (std::f64::consts::PI * z_pixel / (0.5 * h)).cos() + 0.05)
                    * 0.6
                    * (std::f64::consts::PI * dy / dodv).sin();
                v_ratio
            } else {
                0.0
            };
            (wf * profile(h, z_ref), z_pixel)
        }
        WindZone::Upwind => {
            let dy = -(rel_along + half_l);
            let dod = building.upwind_length();
            if dod <= 0.0 {
                return (CDZ * 0.5, h);
            }
            let hd = 0.6 * h * (1.0 - (dy / dod).powi(2)).max(0.0).sqrt();
            let wf = if z_pixel < hd && h > 0.0 {
                CDZ * (z_pixel / h).powf(P_PROFILE)
            } else {
                CDZ * 0.5
            };
            (wf * profile(h, z_ref), z_pixel)
        }
        WindZone::Cavity => {
            let dy = rel_along - half_l;
            let doc = building.cavity_length();
            if doc <= 0.0 || h <= 0.0 {
                return (-0.5 * profile(h.max(0.01), z_ref), z_pixel);
            }
            let hc = h * (1.0 - (dy / doc).powi(2)).max(0.0).sqrt();
            let wf = if z_pixel < hc {
                let term = 1.0 - (dy / doc) * (1.0 - (z_pixel / h).powi(2)).max(0.0).sqrt();
                -(term * term)
            } else {
                -0.5
            };
            (wf * profile(h, z_ref), z_pixel)
        }
        WindZone::Wake => {
            let dy = rel_along - half_l;
            let lr = building.cavity_length();
            let dow = building.wake_length();
            if dow <= lr || dy <= 1e-10 || h <= 0.0 {
                return (1.0 * profile(z_pixel, z_ref), z_pixel);
            }
            let hw = h * (1.0 - (dy / dow).powi(2)).max(0.0).sqrt();
            let wf = if z_pixel < hw {
                let doc = lr;
                let sqrt_inner = (1.0 - (z_pixel / h).powi(2)).max(0.0).sqrt();
                (1.0 - (doc / dy).powf(1.5) * sqrt_inner.powf(1.5)).max(0.0)
            } else {
                1.0
            };
            (wf * profile(z_pixel, z_ref), z_pixel)
        }
        WindZone::Inside => (0.0, z_pixel),
    }
}

/// Speed factor for one zone (multiplier on reference speed; can be negative in cavity).
pub fn zone_speed_factor(
    zone: WindZone,
    rel_along: f64,
    rel_cross: f64,
    building: &RockleBuilding,
    z_pixel: f64,
    z_ref: f64,
    profile_type: WindProfileType,
    z0: f64,
    d: f64,
    hr: f64,
    lambda_f: f64,
) -> f64 {
    let (factor, _) = zone_wind_factor_and_ref(
        zone,
        rel_along,
        rel_cross,
        building,
        z_pixel,
        z_ref,
        profile_type,
        z0,
        d,
        hr,
        lambda_f,
    );
    factor
}

/// Encode WindZone as u8 for rockle_zone.tif: 0=Freestream, 1=Inside, 2=Upwind, 3=UpwindVortex, 4=Cavity, 5=Wake, 6=RooftopPerp, 7=RooftopCorner, 8=StreetCanyon.
fn wind_zone_to_code(zone: WindZone) -> u8 {
    match zone {
        WindZone::Freestream => 0,
        WindZone::Inside => 1,
        WindZone::Upwind => 2,
        WindZone::UpwindVortex => 3,
        WindZone::Cavity => 4,
        WindZone::Wake => 5,
        WindZone::RooftopPerp => 6,
        WindZone::RooftopCorner => 7,
        WindZone::StreetCanyon => 8,
    }
}

// ---------- GeoTransform helper ----------

/// GDAL-style 6-element transform: [x_origin, pixel_width, 0, y_origin, 0, line_height].
pub fn pixel_to_geo(gt: &[f64; 6], col: usize, row: usize) -> (f64, f64) {
    let geo_x = gt[0] + col as f64 * gt[1] + row as f64 * gt[2];
    let geo_y = gt[3] + col as f64 * gt[4] + row as f64 * gt[5];
    (geo_x, geo_y)
}

// ---------- Raster I/O (GDAL) ----------

#[cfg(feature = "gdal")]
fn read_raster_band_as_f32(path: &Path) -> Result<(Array2<f32>, [f64; 6], usize, usize)> {
    let ds = Dataset::open(path).with_context(|| format!("Open raster: {:?}", path))?;
    let (width, height) = ds.raster_size();
    let gt = ds.geo_transform().context("Get geotransform")?;
    let gt_arr: [f64; 6] = [gt[0], gt[1], gt[2], gt[3], gt[4], gt[5]];
    let band = ds.rasterband(1).context("Band 1")?;
    let buf = band
        .read_as::<f32>((0, 0), (width, height), (width, height), None)
        .context("Read band as f32")?;
    let data: Vec<f32> = buf.data().to_vec();
    let arr = Array2::from_shape_vec((height, width), data).context("Shape raster")?;
    Ok((arr, gt_arr, width, height))
}

// ---------- WindConfig ----------

/// Configuration for Röckle wind field and optional mass-consistent solver.
///
/// **Grid spacing (3D solver):**
/// - Horizontal: `solver_dx` / `solver_dy` override the raster pixel size (from geotransform) when set.
/// - Vertical: `solver_dz` overrides the vertical step when set; otherwise `resolution_m` is used as dz.
/// So `resolution_m` is both a general resolution parameter and the default vertical spacing for the solver.
pub struct WindConfig {
    pub wind_speed_ref: f64,
    pub wind_direction: f64,
    pub z_ref: f64,
    /// Nominal resolution [m]. Also used as default vertical step (dz) for the 3D solver when `solver_dz` is None.
    pub resolution_m: f64,
    /// Vertical profile type (URock sect. 2.2).
    pub profile_type: WindProfileType,
    /// Roughness length [m]. If not set, computed from buildings (Hanna & Britter).
    pub z0: Option<f64>,
    /// Displacement height [m]. If not set, computed from buildings.
    pub d: Option<f64>,
    /// Mean obstacle height [m] for urban profile. If not set, computed from buildings.
    pub hr: Option<f64>,
    /// Normalized frontal area for urban profile. If not set, computed from buildings.
    pub lambda_f: Option<f64>,
    /// If true, run mass-consistent solver (Eqs 7–9) on 3D field then extract 2D at output_height.
    pub use_mass_consistent_solver: bool,
    /// Solver stop threshold (sum of |λ^{t+1} − λ^t|).
    pub solver_epsilon: f64,
    /// Max iterations for λ.
    pub solver_max_iter: usize,
    /// Height [m] above ground for 2D output when using solver (slice of 3D result).
    pub output_height: f64,
    /// Grid spacing [m] for solver (x). If None, use raster geotransform.
    pub solver_dx: Option<f64>,
    /// Grid spacing [m] for solver (y). If None, use raster geotransform.
    pub solver_dy: Option<f64>,
    /// Grid spacing [m] for solver (z). If None, use `resolution_m` as vertical step.
    pub solver_dz: Option<f64>,
    /// If true, write a GeoTIFF of Röckle zone codes (rockle_zone.tif): 0=Freestream, 1=Inside, 2=Upwind, 3=UpwindVortex, 4=Cavity, 5=Wake, etc.
    pub save_rockle_zone: bool,
}

impl Default for WindConfig {
    fn default() -> Self {
        Self {
            wind_speed_ref: 3.5,
            wind_direction: 225.0,
            z_ref: 10.0,
            resolution_m: 2.0,
            profile_type: WindProfileType::PowerLaw,
            z0: None,
            d: None,
            hr: None,
            lambda_f: None,
            use_mass_consistent_solver: false,
            solver_epsilon: 0.0001,
            solver_max_iter: 5000,
            output_height: 2.0,
            solver_dx: None,
            solver_dy: None,
            solver_dz: None,
            save_rockle_zone: false,
        }
    }
}

// ---------- Vegetation and building wake ----------

/// True if (geo_x, geo_y, z) lies in the wake zone of any building (for vegetation built-up vs open).
fn point_in_building_wake(
    geo_x: f64,
    geo_y: f64,
    _z_pixel: f64,
    buildings: &[RockleBuilding],
    wind_tf: &WindTransform,
) -> bool {
    let (along, cross) = wind_tf.to_wind_frame(geo_x, geo_y);
    for b in buildings {
        let (b_along, b_cross) = wind_tf.to_wind_frame(b.cx, b.cy);
        let rel_along = along - b_along;
        let rel_cross = cross - b_cross;
        if b.classify_point(rel_along, rel_cross) == WindZone::Wake {
            return true;
        }
    }
    false
}

// ---------- 3D initial field for mass-consistent solver ----------

/// Compute wind speed at one point (geo_x, geo_y, z_above_ground). Used for 3D initial field.
fn speed_at_point(
    geo_x: f64,
    geo_y: f64,
    z_pixel: f64,
    buildings: &[RockleBuilding],
    config: &WindConfig,
    profile: &WindProfileParams,
    wind_tf: &WindTransform,
    vegetation: Option<&VegetationCollection>,
) -> f64 {
    let u_ref = config.wind_speed_ref;
    let z_ref = config.z_ref;
    let pt = profile.profile_type;
    let (z0, d, hr, lambda_f) = (profile.z0, profile.d, profile.hr, profile.lambda_f);
    let (along, cross) = wind_tf.to_wind_frame(geo_x, geo_y);
    let mut min_factor = f64::INFINITY;
    let mut in_any_zone = false;
    for b in buildings {
        let (b_along, b_cross) = wind_tf.to_wind_frame(b.cx, b.cy);
        let rel_along = along - b_along;
        let rel_cross = cross - b_cross;
        let zone = b.classify_point(rel_along, rel_cross);
        if zone != WindZone::Freestream {
            in_any_zone = true;
        }
        let factor = zone_speed_factor(
            zone, rel_along, rel_cross, b, z_pixel, z_ref, pt, z0, d, hr, lambda_f,
        );
        if factor < min_factor {
            min_factor = factor;
        }
    }
    let mut speed = if !in_any_zone || min_factor.is_infinite() || min_factor > 1.0 {
        u_ref * wind_profile_ratio(z_pixel.max(0.1), z_ref, pt, z0, d, hr, lambda_f)
    } else {
        u_ref * min_factor.max(0.0)
    };
    if let Some(veg) = vegetation {
        if let Some(patch) = veg.patch_at(geo_x, geo_y) {
            let in_built_up = point_in_building_wake(geo_x, geo_y, z_pixel, buildings, wind_tf);
            let vf = vegetation_wind_factor(
                z_pixel,
                patch.canopy_height_m,
                patch.z0,
                d,
                patch.alpha_i,
                in_built_up,
            );
            speed *= vf;
        }
    }
    speed
}

/// Build 3D initial field (u0, v0, w0) and solid mask for mass-consistent solver.
/// Grid (nx, ny, nz) = (cols, rows, nz). dz = solver_dz.unwrap_or(resolution_m).
pub fn compute_initial_3d(
    dem: &Array2<f32>,
    dsm: &Array2<f32>,
    buildings: &[RockleBuilding],
    config: &WindConfig,
    profile: &WindProfileParams,
    gt: &[f64; 6],
    wind_tf: &WindTransform,
    vegetation: Option<&VegetationCollection>,
    dz_override: Option<f64>,
) -> (Array3<f64>, Array3<f64>, Array3<f64>, Array3<bool>) {
    let (rows, cols) = dem.dim();
    let dz = dz_override.unwrap_or(config.resolution_m).max(1e-10);
    let z_max_grid = 100.0_f64; // fixed vertical extent for solver
    let nz = (z_max_grid / dz).ceil().max(2.0) as usize;
    let angle_rad = (270.0 - config.wind_direction).to_radians();
    let cos_dir = angle_rad.cos();
    let sin_dir = angle_rad.sin();

    let mut u0 = Array3::zeros((cols, rows, nz));
    let mut v0 = Array3::zeros((cols, rows, nz));
    let mut w0 = Array3::zeros((cols, rows, nz));
    let mut solid = Array3::from_elem((cols, rows, nz), false);

    let indices: Vec<(usize, usize)> = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .collect();
    let results: Vec<((usize, usize), Vec<f64>, Vec<f64>, Vec<f64>, Vec<bool>)> = indices
        .par_iter()
        .map(|&(row, col)| {
            let (geo_x, geo_y) = pixel_to_geo(gt, col, row);
            let dem_val = dem[[row, col]] as f64;
            let dsm_val = dsm[[row, col]] as f64;
            let mut u_vec = vec![0.0; nz];
            let mut v_vec = vec![0.0; nz];
            let w_vec = vec![0.0; nz];
            let mut solid_vec = vec![false; nz];
            for k in 0..nz {
                let z_above_ground = (k as f64 + 0.5) * dz;
                let z_abs = dem_val + z_above_ground;
                if z_abs < dsm_val + 0.01 {
                    solid_vec[k] = true;
                    continue;
                }
                let speed = speed_at_point(
                    geo_x,
                    geo_y,
                    z_above_ground,
                    buildings,
                    config,
                    profile,
                    wind_tf,
                    vegetation,
                );
                u_vec[k] = speed * cos_dir;
                v_vec[k] = speed * sin_dir;
            }
            ((row, col), u_vec, v_vec, w_vec, solid_vec)
        })
        .collect();

    for ((row, col), u_vec, v_vec, w_vec, solid_vec) in results {
        for k in 0..nz {
            u0[[col, row, k]] = u_vec[k];
            v0[[col, row, k]] = v_vec[k];
            w0[[col, row, k]] = w_vec[k];
            solid[[col, row, k]] = solid_vec[k];
        }
    }
    (u0, v0, w0, solid)
}

/// Extract 2D (speed, direction, u, v) from 3D (u, v, w) at height `output_height` above ground per pixel.
fn extract_slice_at_height(
    u: &Array3<f64>,
    v: &Array3<f64>,
    _dem: &Array2<f32>,
    dz: f64,
    output_height: f64,
    wind_direction_deg: f64,
) -> (Array2<f32>, Array2<f32>, Array2<f32>, Array2<f32>) {
    let (nx, ny, nz) = u.dim();
    let indices: Vec<(usize, usize)> = (0..ny)
        .flat_map(|row| (0..nx).map(move |col| (row, col)))
        .collect();
    let results: Vec<(f32, f32, f32, f32)> = indices
        .par_iter()
        .map(|&(row, col)| {
            // Grid is "height above ground" per pixel: level k is at (k+0.5)*dz above local DEM.
            let k_float = (output_height / dz) - 0.5;
            let k0 = k_float.floor().max(0.0).min((nz as f64) - 1.0) as usize;
            let k1 = (k0 + 1).min(nz - 1);
            let t = (k_float - k0 as f64).max(0.0).min(1.0);
            let u_val = u[[col, row, k0]] * (1.0 - t) + u[[col, row, k1]] * t;
            let v_val = v[[col, row, k0]] * (1.0 - t) + v[[col, row, k1]] * t;
            let speed = (u_val * u_val + v_val * v_val).sqrt() as f32;
            let direction = if u_val != 0.0 || v_val != 0.0 {
                (v_val.atan2(u_val).to_degrees() + 180.0).rem_euclid(360.0) as f32
            } else {
                wind_direction_deg as f32
            };
            (speed, direction, u_val as f32, v_val as f32)
        })
        .collect();
    let speed_data: Vec<f32> = results.iter().map(|r| r.0).collect();
    let dir_data: Vec<f32> = results.iter().map(|r| r.1).collect();
    let u_data: Vec<f32> = results.iter().map(|r| r.2).collect();
    let v_data: Vec<f32> = results.iter().map(|r| r.3).collect();
    let speed = Array2::from_shape_vec((ny, nx), speed_data).unwrap();
    let direction = Array2::from_shape_vec((ny, nx), dir_data).unwrap();
    let u_slice = Array2::from_shape_vec((ny, nx), u_data).unwrap();
    let v_slice = Array2::from_shape_vec((ny, nx), v_data).unwrap();
    (speed, direction, u_slice, v_slice)
}

// ---------- compute_wind_field (parallel kernel) ----------

/// Profile parameters used in the kernel (resolved from config and/or buildings).
pub struct WindProfileParams {
    pub profile_type: WindProfileType,
    pub z0: f64,
    pub d: f64,
    pub hr: f64,
    pub lambda_f: f64,
}

/// Compute wind speed and direction rasters. Speed in m/s; direction in degrees.
pub fn compute_wind_field(
    dem: &Array2<f32>,
    dsm: &Array2<f32>,
    buildings: &[RockleBuilding],
    config: &WindConfig,
    profile: &WindProfileParams,
    gt: &[f64; 6],
    wind_tf: &WindTransform,
    vegetation: Option<&VegetationCollection>,
) -> (Array2<f32>, Array2<f32>) {
    let (rows, cols) = dem.dim();
    let u_ref = config.wind_speed_ref;
    let z_ref = config.z_ref;
    let pt = profile.profile_type;
    let z0 = profile.z0;
    let d = profile.d;
    let hr = profile.hr;
    let lambda_f = profile.lambda_f;

    let indices: Vec<(usize, usize)> = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .collect();

    let (speed_data, dir_data): (Vec<f32>, Vec<f32>) = indices
        .par_iter()
        .map(|&(row, col)| {
            let (geo_x, geo_y) = pixel_to_geo(gt, col, row);
            let dsm_val = dsm[[row, col]];
            let dem_val = dem[[row, col]];
            let z_pixel = (dsm_val - dem_val) as f64;
            let z_pixel = z_pixel.max(0.0);
            let (along, cross) = wind_tf.to_wind_frame(geo_x, geo_y);

            let mut min_factor = f64::INFINITY;
            let mut in_any_zone = false;
            for b in buildings {
                let (b_along, b_cross) = wind_tf.to_wind_frame(b.cx, b.cy);
                let rel_along = along - b_along;
                let rel_cross = cross - b_cross;
                let zone = b.classify_point(rel_along, rel_cross);
                if zone != WindZone::Freestream {
                    in_any_zone = true;
                }
                let factor = zone_speed_factor(
                    zone, rel_along, rel_cross, b, z_pixel, z_ref, pt, z0, d, hr, lambda_f,
                );
                if factor < min_factor {
                    min_factor = factor;
                }
            }

            let mut speed = if !in_any_zone || min_factor.is_infinite() || min_factor > 1.0 {
                u_ref * wind_profile_ratio(z_pixel.max(0.1), z_ref, pt, z0, d, hr, lambda_f)
            } else {
                u_ref * min_factor.max(0.0)
            };
            if let Some(veg) = vegetation {
                if let Some(patch) = veg.patch_at(geo_x, geo_y) {
                    let in_built_up =
                        point_in_building_wake(geo_x, geo_y, z_pixel, buildings, wind_tf);
                    let vf = vegetation_wind_factor(
                        z_pixel,
                        patch.canopy_height_m,
                        patch.z0,
                        d,
                        patch.alpha_i,
                        in_built_up,
                    );
                    speed *= vf;
                }
            }
            let direction = config.wind_direction;
            (speed as f32, direction as f32)
        })
        .unzip();

    let speed = Array2::from_shape_vec((rows, cols), speed_data).unwrap();
    let direction = Array2::from_shape_vec((rows, cols), dir_data).unwrap();
    (speed, direction)
}

/// Compute Röckle zone code per pixel (same grid as wind field). For each pixel, the "winning" zone is the building zone (Inside, Cavity, Wake, etc.) that gives the minimum speed factor; Freestream (0) only when the point is not in any building zone.
/// We only assign a non-Freestream zone when the point lies in at least one building zone, so the raster correctly shows cavity/wake/inside where buildings have an effect.
///
/// **Height for classification:** If `height_above_ground` is `Some(h)`, the zone is evaluated at `h` m above ground (same as wind_speed slice at `output_height`). If `None`, the zone is evaluated at the surface height (DSM − DEM) at each pixel.
pub fn compute_rockle_zone_raster(
    dem: &Array2<f32>,
    dsm: &Array2<f32>,
    buildings: &[RockleBuilding],
    config: &WindConfig,
    profile: &WindProfileParams,
    gt: &[f64; 6],
    wind_tf: &WindTransform,
    height_above_ground: Option<f64>,
) -> Array2<u8> {
    let (rows, cols) = dem.dim();
    let z_ref = config.z_ref;
    let pt = profile.profile_type;
    let (z0, d, hr, lambda_f) = (profile.z0, profile.d, profile.hr, profile.lambda_f);

    let indices: Vec<(usize, usize)> = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .collect();

    let zone_data: Vec<u8> = indices
        .par_iter()
        .map(|&(row, col)| {
            let (geo_x, geo_y) = pixel_to_geo(gt, col, row);
            let dsm_val = dsm[[row, col]];
            let dem_val = dem[[row, col]];
            let z_pixel = match height_above_ground {
                Some(h) => h.max(0.0),
                None => (dsm_val - dem_val).max(0.0) as f64,
            };
            let (along, cross) = wind_tf.to_wind_frame(geo_x, geo_y);

            // Only consider building zones (not Freestream) so we assign cavity/wake/inside etc. when the point lies in one
            let mut min_factor = f64::INFINITY;
            let mut min_zone = WindZone::Freestream;
            for b in buildings {
                let (b_along, b_cross) = wind_tf.to_wind_frame(b.cx, b.cy);
                let rel_along = along - b_along;
                let rel_cross = cross - b_cross;
                let zone = b.classify_point(rel_along, rel_cross);
                if zone == WindZone::Freestream {
                    continue;
                }
                let factor = zone_speed_factor(
                    zone, rel_along, rel_cross, b, z_pixel, z_ref, pt, z0, d, hr, lambda_f,
                );
                if factor < min_factor {
                    min_factor = factor;
                    min_zone = zone;
                }
            }
            wind_zone_to_code(min_zone)
        })
        .collect();

    Array2::from_shape_vec((rows, cols), zone_data).unwrap()
}

// ---------- WindFieldResult ----------

pub struct WindFieldResult {
    pub wind_speed_path: std::path::PathBuf,
    pub wind_direction_path: std::path::PathBuf,
    /// Path to rockle_zone.tif when save_rockle_zone was true; None otherwise.
    pub rockle_zone_path: Option<std::path::PathBuf>,
}

// ---------- WindField ----------

pub struct WindField {
    pub geo_core: GeoCore,
    output_path: Option<String>,
    bbox: Option<BoundingBox>,
}

impl WindField {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        let out =
            output_path.or_else(|| Some(crate::collect::global_variables::TEMP_PATH.to_string()));
        Ok(WindField {
            geo_core: GeoCore::default(),
            output_path: out,
            bbox: None,
        })
    }

    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.geo_core
            .set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));
    }

    /// Run wind field: read DEM/DSM from paths, use buildings and optional vegetation, write GeoTIFFs.
    pub fn run(
        &self,
        config: WindConfig,
        dem_path: &Path,
        dsm_path: &Path,
        buildings: &BuildingCollection,
        vegetation: Option<&VegetationCollection>,
    ) -> Result<WindFieldResult> {
        let (dem, gt, _width, _height) = read_raster_band_as_f32(dem_path)?;
        let (dsm, _, _, _) = read_raster_band_as_f32(dsm_path)?;
        if dem.dim() != dsm.dim() {
            anyhow::bail!(
                "DEM and DSM dimensions must match (DEM: {:?}, DSM: {:?})",
                dem.dim(),
                dsm.dim()
            );
        }

        let wind_tf = WindTransform::new(config.wind_direction);
        let rockle = buildings_to_rockle(
            &buildings.buildings,
            &wind_tf,
            buildings.default_storey_height,
        );

        let (rows, cols) = dem.dim();
        let cell_w = gt[1].abs().max(1e-10);
        let cell_h = gt[5].abs().max(1e-10);
        let domain_area = (rows as f64 * cell_h) * (cols as f64 * cell_w);
        let (z0_computed, d_computed, hr_computed, lambda_f_computed) =
            compute_roughness_from_buildings(&rockle, domain_area);
        let profile = WindProfileParams {
            profile_type: config.profile_type,
            z0: config.z0.unwrap_or(z0_computed),
            d: config.d.unwrap_or(d_computed),
            hr: config.hr.unwrap_or(hr_computed),
            lambda_f: config.lambda_f.unwrap_or(lambda_f_computed),
        };

        let (speed, direction, uv_opt) = if config.use_mass_consistent_solver {
            let dz = config.solver_dz.unwrap_or(config.resolution_m).max(1e-10);
            let (u0, v0, w0, solid) = compute_initial_3d(
                &dem,
                &dsm,
                &rockle,
                &config,
                &profile,
                &gt,
                &wind_tf,
                vegetation,
                config.solver_dz,
            );
            let dx = config.solver_dx.unwrap_or_else(|| gt[1].abs().max(1e-10));
            let dy = config.solver_dy.unwrap_or_else(|| gt[5].abs().max(1e-10));
            let (u, v, _w) = solve_mass_consistent(
                &u0,
                &v0,
                &w0,
                &solid,
                dx,
                dy,
                dz,
                1.0,
                1.0,
                config.solver_epsilon,
                config.solver_max_iter,
            );
            let (speed, direction, u_slice, v_slice) = extract_slice_at_height(
                &u,
                &v,
                &dem,
                dz,
                config.output_height,
                config.wind_direction,
            );
            (speed, direction, Some((u_slice, v_slice)))
        } else {
            let (speed, direction) = compute_wind_field(
                &dem, &dsm, &rockle, &config, &profile, &gt, &wind_tf, vegetation,
            );
            (speed, direction, None)
        };

        let out_dir = self
            .output_path
            .as_deref()
            .unwrap_or(crate::collect::global_variables::TEMP_PATH);
        let speed_path = Path::new(out_dir).join("wind_speed.tif");
        let dir_path = Path::new(out_dir).join("wind_direction.tif");

        let speed_desc = format!(
            "Röckle wind speed (m/s), wind_direction_deg={}",
            config.wind_direction
        );
        write_geotiff_f32(
            &speed_path,
            &speed,
            &gt,
            self.geo_core.get_epsg(),
            Some(&speed_desc),
        )?;
        write_geotiff_f32(&dir_path, &direction, &gt, self.geo_core.get_epsg(), None)?;

        if let Some((u_slice, v_slice)) = &uv_opt {
            let dat_path = Path::new(out_dir).join("wind_uv.dat");
            write_wind_uv_dat(&dat_path, &u_slice, &v_slice, &gt, config.output_height)?;
        }

        let rockle_zone_path = if config.save_rockle_zone {
            let zone_raster = compute_rockle_zone_raster(
                &dem,
                &dsm,
                &rockle,
                &config,
                &profile,
                &gt,
                &wind_tf,
                Some(config.output_height),
            );
            let zone_path = Path::new(out_dir).join("rockle_zone.tif");
            let zone_desc = format!(
                "Röckle zone at {:.1} m AGL: 0=Freestream, 1=Inside, 2=Upwind, 3=UpwindVortex, 4=Cavity, 5=Wake, 6=RooftopPerp, 7=RooftopCorner, 8=StreetCanyon",
                config.output_height
            );
            write_geotiff_u8(
                &zone_path,
                &zone_raster,
                &gt,
                self.geo_core.get_epsg(),
                Some(&zone_desc),
            )?;
            Some(zone_path.to_path_buf())
        } else {
            None
        };

        Ok(WindFieldResult {
            wind_speed_path: speed_path.to_path_buf(),
            wind_direction_path: dir_path.to_path_buf(),
            rockle_zone_path,
        })
    }
}

/// Writes velocity fields to a text file: one line per pixel "x y z u v" (world coords, center of pixel).
/// First line: "# width height output_height".
fn write_wind_uv_dat(
    path: &Path,
    u_slice: &Array2<f32>,
    v_slice: &Array2<f32>,
    gt: &[f64; 6],
    output_height: f64,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Create output dir for wind_uv.dat")?;
    }
    let (ny, nx) = u_slice.dim();
    let mut f = std::fs::File::create(path).context("Create wind_uv.dat")?;
    writeln!(f, "# {} {} {}", nx, ny, output_height).context("Write wind_uv.dat header")?;
    for row in 0..ny {
        for col in 0..nx {
            let x = gt[0] + (col as f64 + 0.5) * gt[1] + (row as f64 + 0.5) * gt[2];
            let y = gt[3] + (col as f64 + 0.5) * gt[4] + (row as f64 + 0.5) * gt[5];
            let u = u_slice[[row, col]];
            let v = v_slice[[row, col]];
            writeln!(f, "{} {} {} {} {}", x, y, output_height, u, v)
                .context("Write wind_uv.dat row")?;
        }
    }
    Ok(())
}

#[cfg(feature = "gdal")]
fn write_geotiff_f32(
    path: &Path,
    arr: &Array2<f32>,
    gt: &[f64; 6],
    epsg: i32,
    band_description: Option<&str>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Create output dir")?;
    }
    let (height, width) = arr.dim();
    let driver = gdal::DriverManager::get_driver_by_name("GTiff").context("GTiff driver")?;
    let mut ds = driver
        .create_with_band_type::<f32, _>(path, width, height, 1)
        .context("Create GeoTIFF")?;
    ds.set_geo_transform(gt).context("Set geotransform")?;
    let srs = SpatialRef::from_epsg(epsg as u32).context("SpatialRef")?;
    ds.set_spatial_ref(&srs).context("Set SRS")?;
    let mut band = ds.rasterband(1).context("Band 1")?;
    if let Some(desc) = band_description {
        band.set_description(desc).context("Set band description")?;
    }
    let data: Vec<f32> = arr.iter().copied().collect();
    let mut buf = Buffer::new((width, height), data);
    band.write((0, 0), (width, height), &mut buf)
        .context("Write band")?;
    Ok(())
}

#[cfg(feature = "gdal")]
fn write_geotiff_u8(
    path: &Path,
    arr: &Array2<u8>,
    gt: &[f64; 6],
    epsg: i32,
    band_description: Option<&str>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Create output dir")?;
    }
    let (height, width) = arr.dim();
    let driver = gdal::DriverManager::get_driver_by_name("GTiff").context("GTiff driver")?;
    let mut ds = driver
        .create_with_band_type::<u8, _>(path, width, height, 1)
        .context("Create GeoTIFF")?;
    ds.set_geo_transform(gt).context("Set geotransform")?;
    let srs = SpatialRef::from_epsg(epsg as u32).context("SpatialRef")?;
    ds.set_spatial_ref(&srs).context("Set SRS")?;
    let mut band = ds.rasterband(1).context("Band 1")?;
    if let Some(desc) = band_description {
        band.set_description(desc).context("Set band description")?;
    }
    let data: Vec<u8> = arr.iter().copied().collect();
    let mut buf = Buffer::new((width, height), data);
    band.write((0, 0), (width, height), &mut buf)
        .context("Write band")?;
    Ok(())
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_transform_roundtrip() {
        let tf = WindTransform::new(225.0);
        let (x, y) = (100.0, 200.0);
        let (along, cross) = tf.to_wind_frame(x, y);
        let (x2, y2) = tf.from_wind_frame(along, cross);
        assert!((x - x2).abs() < 1e-10);
        assert!((y - y2).abs() < 1e-10);
    }

    #[test]
    fn rockle_building_zone_formulas() {
        let b = RockleBuilding {
            cx: 0.0,
            cy: 0.0,
            h: 10.0,
            w: 20.0,
            l: 15.0,
            weff: 20.0,
            leff: 15.0,
        };
        assert!(b.upwind_length() > 0.0);
        assert!(b.upwind_vortex_length() > 0.0);
        assert!(b.upwind_vortex_length() < b.upwind_length());
        assert!(b.cavity_length() > 0.0);
        assert_eq!(b.wake_length(), 3.0 * b.cavity_length());
        assert!(b.displacement_height() > 0.0);
    }

    #[test]
    fn rockle_classify_inside_and_freestream() {
        let b = RockleBuilding {
            cx: 0.0,
            cy: 0.0,
            h: 10.0,
            w: 20.0,
            l: 15.0,
            weff: 20.0,
            leff: 15.0,
        };
        assert_eq!(b.classify_point(0.0, 0.0), WindZone::Inside);
        assert_eq!(b.classify_point(-100.0, 0.0), WindZone::Freestream);
        assert_eq!(b.classify_point(100.0, 100.0), WindZone::Freestream);
    }

    #[test]
    fn rockle_classify_upwind_cavity_wake() {
        let b = RockleBuilding {
            cx: 0.0,
            cy: 0.0,
            h: 10.0,
            w: 20.0,
            l: 15.0,
            weff: 20.0,
            leff: 15.0,
        };
        let half_l = 7.5;
        let lu = b.upwind_length();
        assert!(lu > 0.0);
        assert_eq!(b.classify_point(-half_l - lu * 0.5, 0.0), WindZone::Upwind);
        let lr = b.cavity_length();
        assert_eq!(b.classify_point(half_l + lr * 0.5, 0.0), WindZone::Cavity);
        let lw = b.wake_length();
        assert_eq!(
            b.classify_point(half_l + lr + (lw - lr) * 0.5, 0.0),
            WindZone::Wake
        );
        // Displacement vortex: point close to facade (inside smaller ellipse).
        let lfv = b.upwind_vortex_length();
        assert_eq!(
            b.classify_point(-half_l - lfv * 0.5, 0.0),
            WindZone::UpwindVortex
        );
    }

    #[test]
    fn log_wind_profile_bounds() {
        assert!((log_wind_profile(10.0, 10.0) - 1.0).abs() < 1e-6);
        assert!(log_wind_profile(20.0, 10.0) > 1.0);
        assert!(log_wind_profile(5.0, 10.0) < 1.0 && log_wind_profile(5.0, 10.0) > 0.0);
    }
}
