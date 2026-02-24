//! Röckle-based urban wind field: analytical zones (upwind, cavity, wake) and parallel raster solver.
//!
//! References: Röckle (1990), Kaplan & Dinar (1996), URock (Bernard et al., 2023).

use anyhow::{Context, Result};
use ndarray::Array2;
use std::path::Path;

use crate::geo_core::{BoundingBox, GeoCore};
use crate::geometric::building::{Building, BuildingCollection};

#[cfg(feature = "gdal")]
use gdal::Dataset;
#[cfg(feature = "gdal")]
use gdal::raster::Buffer;
#[cfg(feature = "gdal")]
use gdal::spatial_ref::SpatialRef;
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
    Upwind,
    Cavity,
    Wake,
    Freestream,
}

// ---------- RockleBuilding ----------

/// One building in wind-aligned frame: centre (cx, cy), height h, width W (cross-wind), length L (along-wind).
pub struct RockleBuilding {
    pub cx: f64,
    pub cy: f64,
    pub h: f64,
    pub w: f64,
    pub l: f64,
}

impl RockleBuilding {
    /// Upwind deflection zone length (Röckle): Lu = min(H, W/2) * 0.9
    pub fn upwind_length(&self) -> f64 {
        self.h.min(self.w / 2.0) * 0.9
    }

    /// Cavity (recirculation) length (Kaplan & Dinar 1996): Lr = 1.8*W / (H/W + 0.24)
    pub fn cavity_length(&self) -> f64 {
        let r = self.h / self.w;
        1.8 * self.w / (r + 0.24)
    }

    /// Wake length: Lw = Lr * (1 + 0.5*H/W)
    pub fn wake_length(&self) -> f64 {
        self.cavity_length() * (1.0 + 0.5 * self.h / self.w)
    }

    /// Displacement zone height: Hd = H + 0.22*sqrt(H*W)
    pub fn displacement_height(&self) -> f64 {
        self.h + 0.22 * (self.h * self.w).sqrt()
    }

    /// Classify point (relative to building centre, in wind frame) into a zone.
    pub fn classify_point(&self, rel_along: f64, rel_cross: f64) -> WindZone {
        let half_l = self.l / 2.0;
        let half_w = self.w / 2.0;
        let lu = self.upwind_length();
        let lr = self.cavity_length();
        let lw = self.wake_length();

        if rel_along.abs() <= half_l && rel_cross.abs() <= half_w {
            return WindZone::Inside;
        }

        if rel_along < -half_l && rel_along > -(half_l + lu) && rel_cross.abs() < half_w {
            return WindZone::Upwind;
        }

        let dist_cavity = rel_along - half_l;
        if dist_cavity > 0.0 && dist_cavity < lr {
            let half_width_cavity = half_w + 0.5 * (lr - dist_cavity);
            if rel_cross.abs() < half_width_cavity {
                return WindZone::Cavity;
            }
        }

        if dist_cavity >= lr && dist_cavity < lw {
            let half_width_wake = half_w + dist_cavity * (self.displacement_height() / lw);
            if rel_cross.abs() < half_width_wake {
                return WindZone::Wake;
            }
        }

        WindZone::Freestream
    }
}

/// Build RockleBuilding list from BuildingCollection and wind direction.
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
            let (a, c) = wind_tf.to_wind_frame(coord.x, coord.y);
            min_along = min_along.min(a);
            max_along = max_along.max(a);
            min_cross = min_cross.min(c);
            max_cross = max_cross.max(c);
        }
        let l = (max_along - min_along).max(0.1);
        let w = (max_cross - min_cross).max(0.1);
        out.push(RockleBuilding {
            cx,
            cy,
            h,
            w,
            l,
        });
    }
    out
}

// ---------- Wind profile and zone speed factor ----------

const Z0_URBAN: f64 = 0.5;

/// Logarithmic wind profile: U(z)/U(z_ref) = ln(z/z0) / ln(z_ref/z0).
pub fn log_wind_profile(z: f64, z_ref: f64) -> f64 {
    let z = z.max(0.01);
    let z_ref = z_ref.max(0.01);
    (z / Z0_URBAN).ln() / (z_ref / Z0_URBAN).ln()
}

/// Speed factor for one zone (multiplier on reference speed; can be negative in cavity).
pub fn zone_speed_factor(
    zone: WindZone,
    rel_along: f64,
    _rel_cross: f64,
    building: &RockleBuilding,
    z_pixel: f64,
    z_ref: f64,
) -> f64 {
    match zone {
        WindZone::Freestream => {
            if z_pixel > building.h {
                log_wind_profile(z_pixel, z_ref)
            } else {
                1.0
            }
        }
        WindZone::Upwind => {
            let dist = -(rel_along + building.l / 2.0);
            let lu = building.upwind_length();
            if lu <= 0.0 {
                0.5
            } else {
                0.5 * (1.0 - dist / lu)
            }
        }
        WindZone::Cavity => {
            let dist = rel_along - building.l / 2.0;
            let lr = building.cavity_length();
            if lr <= 0.0 {
                -0.5
            } else {
                -0.5 * (1.0 - dist / lr)
            }
        }
        WindZone::Wake => {
            let dist = rel_along - building.l / 2.0;
            let lw = building.wake_length();
            let lr = building.cavity_length();
            if lw <= lr {
                1.0
            } else {
                (dist - lr) / (lw - lr)
            }
        }
        WindZone::Inside => 0.0,
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

pub struct WindConfig {
    pub wind_speed_ref: f64,
    pub wind_direction: f64,
    pub z_ref: f64,
    pub resolution_m: f64,
}

// ---------- compute_wind_field (parallel kernel) ----------

/// Compute wind speed and direction rasters. Speed in m/s; direction in degrees (constant in V1).
pub fn compute_wind_field(
    dem: &Array2<f32>,
    dsm: &Array2<f32>,
    buildings: &[RockleBuilding],
    config: &WindConfig,
    gt: &[f64; 6],
    wind_tf: &WindTransform,
) -> (Array2<f32>, Array2<f32>) {
    let (rows, cols) = dem.dim();
    let u_ref = config.wind_speed_ref;
    let z_ref = config.z_ref;

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
                let factor = zone_speed_factor(zone, rel_along, rel_cross, b, z_pixel, z_ref);
                if factor < min_factor {
                    min_factor = factor;
                }
            }

            let speed = if !in_any_zone || min_factor.is_infinite() || min_factor > 1.0 {
                u_ref * log_wind_profile(z_pixel.max(0.1), z_ref)
            } else {
                u_ref * min_factor.max(0.0)
            };
            let direction = config.wind_direction;
            (speed as f32, direction as f32)
        })
        .unzip();

    let speed = Array2::from_shape_vec((rows, cols), speed_data).unwrap();
    let direction = Array2::from_shape_vec((rows, cols), dir_data).unwrap();
    (speed, direction)
}

// ---------- WindFieldResult ----------

pub struct WindFieldResult {
    pub wind_speed_path: std::path::PathBuf,
    pub wind_direction_path: std::path::PathBuf,
}

// ---------- WindField ----------

pub struct WindField {
    pub geo_core: GeoCore,
    output_path: Option<String>,
    bbox: Option<BoundingBox>,
}

impl WindField {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        let out = output_path.or_else(|| Some(crate::collect::global_variables::TEMP_PATH.to_string()));
        Ok(WindField {
            geo_core: GeoCore::default(),
            output_path: out,
            bbox: None,
        })
    }

    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.geo_core.set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));
    }

    /// Run wind field: read DEM/DSM from paths, use buildings from collection, write GeoTIFFs.
    pub fn run(
        &self,
        config: WindConfig,
        dem_path: &Path,
        dsm_path: &Path,
        buildings: &BuildingCollection,
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

        let (speed, direction) = compute_wind_field(&dem, &dsm, &rockle, &config, &gt, &wind_tf);

        let out_dir = self
            .output_path
            .as_deref()
            .unwrap_or(crate::collect::global_variables::TEMP_PATH);
        let speed_path = Path::new(out_dir).join("wind_speed.tif");
        let dir_path = Path::new(out_dir).join("wind_direction.tif");

        write_geotiff_f32(&speed_path, &speed, &gt, self.geo_core.get_epsg())?;
        write_geotiff_f32(&dir_path, &direction, &gt, self.geo_core.get_epsg())?;

        Ok(WindFieldResult {
            wind_speed_path: speed_path.to_path_buf(),
            wind_direction_path: dir_path.to_path_buf(),
        })
    }
}

#[cfg(feature = "gdal")]
fn write_geotiff_f32(
    path: &Path,
    arr: &Array2<f32>,
    gt: &[f64; 6],
    epsg: i32,
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
    let data: Vec<f32> = arr.iter().copied().collect();
    let mut buf = Buffer::new((width, height), data);
    band.write((0, 0), (width, height), &mut buf).context("Write band")?;
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
        };
        assert!(b.upwind_length() > 0.0);
        assert!(b.cavity_length() > 0.0);
        assert!(b.wake_length() > b.cavity_length());
        assert!(b.displacement_height() > b.h);
    }

    #[test]
    fn rockle_classify_inside_and_freestream() {
        let b = RockleBuilding {
            cx: 0.0,
            cy: 0.0,
            h: 10.0,
            w: 20.0,
            l: 15.0,
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
        };
        let half_l = 7.5;
        let lu = b.upwind_length();
        assert!(lu > 0.0);
        assert_eq!(
            b.classify_point(-half_l - lu * 0.5, 0.0),
            WindZone::Upwind
        );
        let lr = b.cavity_length();
        assert_eq!(
            b.classify_point(half_l + lr * 0.5, 0.0),
            WindZone::Cavity
        );
        let lw = b.wake_length();
        assert_eq!(
            b.classify_point(half_l + lr + (lw - lr) * 0.5, 0.0),
            WindZone::Wake
        );
    }

    #[test]
    fn log_wind_profile_bounds() {
        assert!((log_wind_profile(10.0, 10.0) - 1.0).abs() < 1e-6);
        assert!(log_wind_profile(20.0, 10.0) > 1.0);
        assert!(log_wind_profile(5.0, 10.0) < 1.0 && log_wind_profile(5.0, 10.0) > 0.0);
    }
}
