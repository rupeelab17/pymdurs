//! CPU rasterization, DTM hole filling and CHM.
//!
//! Reference and fallback for `gpu.rs`: same semantics, same Jump Flooding schedule and
//! tie-break, so CPU and GPU outputs agree (up to f32 rounding of Z on the GPU side).
//! All rasters are flat, row-major (`row * width + col`).

use super::types::{DtmFill, LidarPoint};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

pub(super) const NO_SEED: u32 = u32::MAX;

/// 256-bit classification filter, shared by the CPU loop and the GPU kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClassMask([u32; 8]);

impl ClassMask {
    pub(super) fn all() -> Self {
        Self([u32::MAX; 8])
    }

    /// `None` → every class; `Some(list)` → only the listed classes.
    pub(super) fn from_list(list: Option<&[u8]>) -> Self {
        match list {
            None => Self::all(),
            Some(classes) => {
                let mut words = [0u32; 8];
                for &c in classes {
                    words[(c >> 5) as usize] |= 1 << (c & 31);
                }
                Self(words)
            }
        }
    }

    #[inline]
    pub(super) fn contains(&self, c: u8) -> bool {
        self.0[(c >> 5) as usize] & (1 << (c & 31)) != 0
    }

    #[cfg_attr(not(feature = "gpu"), allow(dead_code))]
    pub(super) fn words(&self) -> [u32; 8] {
        self.0
    }
}

/// Max-Z binning: DSM (enabled classes) and ground DTM (class 2). Empty cells are `-inf`.
pub(super) fn rasterize_max_z_cpu(
    points: &[LidarPoint],
    mask: &ClassMask,
    x_min: f64,
    y_max: f64,
    resolution: f64,
    width: usize,
    height: usize,
) -> (Vec<f64>, Vec<f64>) {
    let mut dsm = vec![f64::NEG_INFINITY; width * height];
    let mut dtm = vec![f64::NEG_INFINITY; width * height];
    for p in points {
        if !mask.contains(p.classification) {
            continue;
        }
        let fx = (p.x - x_min) / resolution;
        let fy = (y_max - p.y) / resolution;
        // `as usize` saturates negatives to 0: reject them explicitly (also rejects NaN).
        if !(fx >= 0.0 && fy >= 0.0) {
            continue;
        }
        let (col, row) = (fx.floor() as usize, fy.floor() as usize);
        if col >= width || row >= height {
            continue;
        }
        let i = row * width + col;
        if p.z > dsm[i] {
            dsm[i] = p.z;
        }
        if p.classification == 2 && p.z > dtm[i] {
            dtm[i] = p.z;
        }
    }
    (dsm, dtm)
}

/// Fill DTM cells without ground points.
pub(super) fn fill_dtm_cpu(dtm: &[f64], width: usize, height: usize, mode: DtmFill) -> Vec<f64> {
    match mode {
        DtmFill::Neighbor3x3 => fill_neighbor3x3(dtm, width, height),
        DtmFill::Nearest => {
            let seeds = nearest_ground_seeds(dtm, width, height);
            dtm.iter()
                .zip(&seeds)
                .map(|(&v, &s)| {
                    if v != f64::NEG_INFINITY {
                        v
                    } else if s == NO_SEED {
                        0.0 // no ground point in the whole raster
                    } else {
                        dtm[s as usize]
                    }
                })
                .collect()
        }
    }
}

/// Legacy behaviour: one pass, min of the 3×3 ground neighbours, 0.0 if none.
fn fill_neighbor3x3(dtm: &[f64], width: usize, height: usize) -> Vec<f64> {
    let mut filled = dtm.to_vec();
    for row in 0..height {
        for col in 0..width {
            let i = row * width + col;
            if dtm[i] != f64::NEG_INFINITY {
                continue;
            }
            let mut min_neighbor = f64::INFINITY;
            for dr in -1i64..=1 {
                for dc in -1i64..=1 {
                    let (r, c) = (row as i64 + dr, col as i64 + dc);
                    if r >= 0 && r < height as i64 && c >= 0 && c < width as i64 {
                        let v = dtm[r as usize * width + c as usize];
                        if v != f64::NEG_INFINITY && v < min_neighbor {
                            min_neighbor = v;
                        }
                    }
                }
            }
            filled[i] = if min_neighbor != f64::INFINITY {
                min_neighbor
            } else {
                0.0
            };
        }
    }
    filled
}

/// Jump Flooding schedule: N/2, N/4, …, 1, then one extra pass at 1 ("JFA+1").
/// Must match the GPU dispatch order exactly.
pub(super) fn jfa_steps(width: usize, height: usize) -> Vec<u32> {
    let n = width.max(height).max(1).next_power_of_two();
    let mut steps = Vec::new();
    let mut k = n / 2;
    while k >= 1 {
        steps.push(k as u32);
        k /= 2;
    }
    steps.push(1);
    steps
}

/// For each cell, index of the nearest ground cell (Euclidean, JFA approximation),
/// `NO_SEED` if the raster has no ground at all.
pub(super) fn nearest_ground_seeds(dtm: &[f64], width: usize, height: usize) -> Vec<u32> {
    let mut src: Vec<u32> = dtm
        .iter()
        .enumerate()
        .map(|(i, &v)| if v == f64::NEG_INFINITY { NO_SEED } else { i as u32 })
        .collect();
    if width == 0 || height == 0 {
        return src;
    }
    let mut dst = vec![NO_SEED; src.len()];
    for k in jfa_steps(width, height) {
        {
            let src_ref = &src;
            let step_row = |(y, out): (usize, &mut [u32])| {
                jfa_row(src_ref, out, y, width, height, k as i64)
            };
            #[cfg(feature = "rayon")]
            dst.par_chunks_mut(width).enumerate().for_each(step_row);
            #[cfg(not(feature = "rayon"))]
            dst.chunks_mut(width).enumerate().for_each(step_row);
        }
        std::mem::swap(&mut src, &mut dst);
    }
    src
}

fn jfa_row(src: &[u32], out: &mut [u32], y: usize, width: usize, height: usize, k: i64) {
    let (w, h, y) = (width as i64, height as i64, y as i64);
    for (x, slot) in out.iter_mut().enumerate() {
        let x = x as i64;
        let mut best = NO_SEED;
        let mut best_d = u64::MAX;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (nx, ny) = (x + dx * k, y + dy * k);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let s = src[(ny * w + nx) as usize];
                if s == NO_SEED {
                    continue;
                }
                let (sx, sy) = (s as i64 % w, s as i64 / w);
                let d = ((sx - x) * (sx - x) + (sy - y) * (sy - y)) as u64;
                // Same tie-break as the WGSL kernel: smallest seed index.
                if d < best_d || (d == best_d && s < best) {
                    best = s;
                    best_d = d;
                }
            }
        }
        *slot = best;
    }
}

/// CHM = max(DSM − DTM, 0); 0 where the DSM is empty.
pub(super) fn chm_cpu(dsm: &[f64], dtm_filled: &[f64]) -> Vec<f64> {
    dsm.iter()
        .zip(dtm_filled)
        .map(|(&s, &t)| {
            if s == f64::NEG_INFINITY {
                0.0
            } else {
                (s - t).max(0.0)
            }
        })
        .collect()
}

