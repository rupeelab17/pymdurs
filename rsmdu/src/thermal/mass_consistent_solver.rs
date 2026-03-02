//! Mass-consistent wind field solver (URock sect. 2.4.2, Eqs 7–9).
//!
//! Minimizes E(u,v,w,λ) with constraint div(u)=0 via Lagrange multiplier λ.
//! ∇²λ = -2α² div(u0); u = u0 + ∂λ/∂x/(2α1²), v = v0 + ∂λ/∂y/(2α1²), w = w0 + ∂λ/∂z/(2α2²).
//! BC: ∂λ/∂n = 0 on solid walls, λ = 0 on domain boundary.
//!
//! Uses red-black SOR (Successive Over-Relaxation) for faster convergence than Jacobi.

use ndarray::Array3;
use rayon::prelude::*;

#[cfg(feature = "indicatif")]
use indicatif::{ProgressBar, ProgressStyle};

/// SOR relaxation parameter. ω = 1 gives red-black Gauss–Seidel (stable).
/// ω > 1 can diverge with Neumann BC and irregular solids; use 1.0 for safety.
const SOR_OMEGA: f64 = 1.0;

/// Solve for λ such that u = u0 + grad(λ)/(2α²) is divergence-free.
/// Cell-centered u0,v0,w0 and λ; face fluxes from interpolation.
/// solid[i,j,k] = true means obstacle (no flow); λ not solved there, Neumann ∂λ/∂n=0 at interface.
pub fn solve_mass_consistent(
    u0: &Array3<f64>,
    v0: &Array3<f64>,
    w0: &Array3<f64>,
    solid: &Array3<bool>,
    dx: f64,
    dy: f64,
    dz: f64,
    alpha1: f64,
    alpha2: f64,
    epsilon: f64,
    max_iter: usize,
) -> (Array3<f64>, Array3<f64>, Array3<f64>) {
    let (nx, ny, nz) = u0.dim();
    let mut lam = Array3::<f64>::zeros((nx, ny, nz));
    let a1 = alpha1.max(1e-10);
    let a2 = alpha2.max(1e-10);
    let two_a1_sq = 2.0 * a1 * a1;
    let two_a2_sq = 2.0 * a2 * a2;

    let inv_dx2 = 1.0 / (dx * dx);
    let inv_dy2 = 1.0 / (dy * dy);
    let inv_dz2 = 1.0 / (dz * dz);
    let diag = -2.0 * (inv_dx2 + inv_dy2 + inv_dz2);

    let fluid_indices: Vec<(usize, usize, usize)> = (1..nx - 1)
        .flat_map(|i| (1..ny - 1).flat_map(move |j| (1..nz - 1).map(move |k| (i, j, k))))
        .filter(|&(i, j, k)| !solid[[i, j, k]])
        .collect();

    let red_indices: Vec<(usize, usize, usize)> = fluid_indices
        .iter()
        .filter(|&&(i, j, k)| (i + j + k) % 2 == 0)
        .copied()
        .collect();
    let black_indices: Vec<(usize, usize, usize)> = fluid_indices
        .iter()
        .filter(|&&(i, j, k)| (i + j + k) % 2 == 1)
        .copied()
        .collect();

    #[cfg(feature = "indicatif")]
    let pb = {
        let pb = ProgressBar::new(max_iter as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} λ solver {msg}")
                .unwrap()
                .progress_chars("##-"),
        );
        pb
    };

    let mut iter = 0;
    while iter < max_iter {
        let mut lam_new = lam.clone();

        // Pass 1: update red cells (neighbors are black or boundary → read from lam)
        let (red_updates, max_diff_red): (Vec<_>, f64) = red_indices
            .par_iter()
            .map(|&(i, j, k)| {
                let ue = 0.5 * (u0[[i, j, k]] + u0[[i + 1, j, k]]);
                let uw = 0.5 * (u0[[i - 1, j, k]] + u0[[i, j, k]]);
                let vn = 0.5 * (v0[[i, j, k]] + v0[[i, j + 1, k]]);
                let vs = 0.5 * (v0[[i, j - 1, k]] + v0[[i, j, k]]);
                let wt = 0.5 * (w0[[i, j, k]] + w0[[i, j, k + 1]]);
                let wb = 0.5 * (w0[[i, j, k - 1]] + w0[[i, j, k]]);
                let div_u0 = (ue - uw) / dx + (vn - vs) / dy + (wt - wb) / dz;

                let lip1 = if solid[[i + 1, j, k]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i + 1, j, k]]
                };
                let lim1 = if solid[[i - 1, j, k]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i - 1, j, k]]
                };
                let ljp1 = if solid[[i, j + 1, k]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i, j + 1, k]]
                };
                let ljm1 = if solid[[i, j - 1, k]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i, j - 1, k]]
                };
                let lkp1 = if solid[[i, j, k + 1]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i, j, k + 1]]
                };
                let lkm1 = if solid[[i, j, k - 1]] {
                    lam[[i, j, k]]
                } else {
                    lam[[i, j, k - 1]]
                };

                let rhs = -two_a1_sq * div_u0;
                let lam_star = (rhs
                    + (lip1 + lim1) * inv_dx2
                    + (ljp1 + ljm1) * inv_dy2
                    + (lkp1 + lkm1) * inv_dz2)
                    / diag;
                let new_val = (1.0 - SOR_OMEGA) * lam[[i, j, k]] + SOR_OMEGA * lam_star;
                let diff = (new_val - lam[[i, j, k]]).abs();
                ((i, j, k), new_val, diff)
            })
            .fold(
                || (Vec::new(), 0.0_f64),
                |(mut acc, max_d), ((i, j, k), v, d)| {
                    acc.push(((i, j, k), v));
                    (acc, max_d.max(d))
                },
            )
            .reduce(
                || (Vec::new(), 0.0),
                |(mut a, md_a), (b, md_b)| {
                    a.extend(b);
                    (a, md_a.max(md_b))
                },
            );

        for ((i, j, k), v) in red_updates {
            lam_new[[i, j, k]] = v;
        }

        // Pass 2: update black cells (red neighbors from lam_new, black from lam)
        let (black_updates, max_diff_black): (Vec<_>, f64) = black_indices
            .par_iter()
            .map(|&(i, j, k)| {
                let ue = 0.5 * (u0[[i, j, k]] + u0[[i + 1, j, k]]);
                let uw = 0.5 * (u0[[i - 1, j, k]] + u0[[i, j, k]]);
                let vn = 0.5 * (v0[[i, j, k]] + v0[[i, j + 1, k]]);
                let vs = 0.5 * (v0[[i, j - 1, k]] + v0[[i, j, k]]);
                let wt = 0.5 * (w0[[i, j, k]] + w0[[i, j, k + 1]]);
                let wb = 0.5 * (w0[[i, j, k - 1]] + w0[[i, j, k]]);
                let div_u0 = (ue - uw) / dx + (vn - vs) / dy + (wt - wb) / dz;

                let lip1 = if solid[[i + 1, j, k]] {
                    lam[[i, j, k]]
                } else if (i + 1 + j + k) % 2 == 0 {
                    lam_new[[i + 1, j, k]]
                } else {
                    lam[[i + 1, j, k]]
                };
                let lim1 = if solid[[i - 1, j, k]] {
                    lam[[i, j, k]]
                } else if (i - 1 + j + k) % 2 == 0 {
                    lam_new[[i - 1, j, k]]
                } else {
                    lam[[i - 1, j, k]]
                };
                let ljp1 = if solid[[i, j + 1, k]] {
                    lam[[i, j, k]]
                } else if (i + j + 1 + k) % 2 == 0 {
                    lam_new[[i, j + 1, k]]
                } else {
                    lam[[i, j + 1, k]]
                };
                let ljm1 = if solid[[i, j - 1, k]] {
                    lam[[i, j, k]]
                } else if (i + j - 1 + k) % 2 == 0 {
                    lam_new[[i, j - 1, k]]
                } else {
                    lam[[i, j - 1, k]]
                };
                let lkp1 = if solid[[i, j, k + 1]] {
                    lam[[i, j, k]]
                } else if (i + j + k + 1) % 2 == 0 {
                    lam_new[[i, j, k + 1]]
                } else {
                    lam[[i, j, k + 1]]
                };
                let lkm1 = if solid[[i, j, k - 1]] {
                    lam[[i, j, k]]
                } else if (i + j + k - 1) % 2 == 0 {
                    lam_new[[i, j, k - 1]]
                } else {
                    lam[[i, j, k - 1]]
                };

                let rhs = -two_a1_sq * div_u0;
                let lam_star = (rhs
                    + (lip1 + lim1) * inv_dx2
                    + (ljp1 + ljm1) * inv_dy2
                    + (lkp1 + lkm1) * inv_dz2)
                    / diag;
                let new_val = (1.0 - SOR_OMEGA) * lam[[i, j, k]] + SOR_OMEGA * lam_star;
                let diff = (new_val - lam[[i, j, k]]).abs();
                ((i, j, k), new_val, diff)
            })
            .fold(
                || (Vec::new(), 0.0_f64),
                |(mut acc, max_d), ((i, j, k), v, d)| {
                    acc.push(((i, j, k), v));
                    (acc, max_d.max(d))
                },
            )
            .reduce(
                || (Vec::new(), 0.0),
                |(mut a, md_a), (b, md_b)| {
                    a.extend(b);
                    (a, md_a.max(md_b))
                },
            );

        for ((i, j, k), v) in black_updates {
            lam_new[[i, j, k]] = v;
        }

        let max_diff = max_diff_red.max(max_diff_black);
        if !max_diff.is_finite() || max_diff > 1e30 {
            #[cfg(feature = "indicatif")]
            pb.finish_with_message("diverged (keeping last λ)");
            break;
        }
        lam = lam_new;

        #[cfg(feature = "indicatif")]
        {
            pb.set_position(iter as u64);
            pb.set_message(format!("max_diff = {:.2e}", max_diff));
        }

        if max_diff < epsilon {
            #[cfg(feature = "indicatif")]
            pb.finish_with_message("converged");
            break;
        }
        iter += 1;
    }

    #[cfg(feature = "indicatif")]
    if iter >= max_iter {
        pb.finish_with_message("max iter reached");
    }

    // u = u0 + ∂λ/∂x/(2α1²), etc. (λ = 0 on domain boundary)
    let mut u = u0.clone();
    let mut v = v0.clone();
    let mut w = w0.clone();

    let uvww_updates: Vec<(usize, Vec<f64>, Vec<f64>, Vec<f64>)> = (0..ny)
        .into_par_iter()
        .map(|j| {
            let mut u_row = vec![0.0; nx * nz];
            let mut v_row = vec![0.0; nx * nz];
            let mut w_row = vec![0.0; nx * nz];
            for i in 0..nx {
                for k in 0..nz {
                    let idx = i * nz + k;
                    if solid[[i, j, k]] {
                        u_row[idx] = 0.0;
                        v_row[idx] = 0.0;
                        w_row[idx] = 0.0;
                        continue;
                    }
                    let lam_c = lam[[i, j, k]];
                    let lip1 = if i + 1 < nx { lam[[i + 1, j, k]] } else { 0.0 };
                    let lim1 = if i > 0 { lam[[i - 1, j, k]] } else { 0.0 };
                    let ljp1 = if j + 1 < ny { lam[[i, j + 1, k]] } else { 0.0 };
                    let ljm1 = if j > 0 { lam[[i, j - 1, k]] } else { 0.0 };
                    let lkp1 = if k + 1 < nz { lam[[i, j, k + 1]] } else { 0.0 };
                    let lkm1 = if k > 0 { lam[[i, j, k - 1]] } else { 0.0 };
                    let dl_dx = if i > 0 && i + 1 < nx {
                        (lip1 - lim1) / (2.0 * dx)
                    } else if i + 1 < nx {
                        (lip1 - lam_c) / dx
                    } else {
                        (lam_c - lim1) / dx
                    };
                    let dl_dy = if j > 0 && j + 1 < ny {
                        (ljp1 - ljm1) / (2.0 * dy)
                    } else if j + 1 < ny {
                        (ljp1 - lam_c) / dy
                    } else {
                        (lam_c - ljm1) / dy
                    };
                    let dl_dz = if k > 0 && k + 1 < nz {
                        (lkp1 - lkm1) / (2.0 * dz)
                    } else if k + 1 < nz {
                        (lkp1 - lam_c) / dz
                    } else {
                        (lam_c - lkm1) / dz
                    };
                    u_row[idx] = u0[[i, j, k]] + dl_dx / two_a1_sq;
                    v_row[idx] = v0[[i, j, k]] + dl_dy / two_a1_sq;
                    w_row[idx] = w0[[i, j, k]] + dl_dz / two_a2_sq;
                }
            }
            (j, u_row, v_row, w_row)
        })
        .collect();

    for (j, u_row, v_row, w_row) in uvww_updates {
        for i in 0..nx {
            for k in 0..nz {
                let idx = i * nz + k;
                u[[i, j, k]] = u_row[idx];
                v[[i, j, k]] = v_row[idx];
                w[[i, j, k]] = w_row[idx];
            }
        }
    }
    (u, v, w)
}
