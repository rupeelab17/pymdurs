use std::path::{Path, PathBuf};

use super::copc::*;
use super::io::*;
use super::raster::{
    chm_cpu, fill_dtm_cpu, nearest_ground_seeds, rasterize_max_z_cpu, ClassMask, NO_SEED,
};
use super::spatial::*;
use super::types::{classification_to_u8, DtmFill, Lidar, LidarPoint};

#[cfg(feature = "gpu")]
use super::gpu::rasterize_fill_chm_gpu_batched;

/// Deterministic pseudo-random generator (no `rand` dependency in tests).
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next_f64() * n as f64) as usize).min(n - 1)
    }
}

/// Points near cell centres (no rounding ambiguity at cell edges), with a central
/// "building" where no ground point exists.
fn synthetic_points(n: usize, width: usize, height: usize, res: f64) -> Vec<LidarPoint> {
    let classes = [1u8, 2, 3, 5, 6, 7, 66];
    let mut rng = Lcg(42);
    (0..n)
        .map(|_| {
            let (cx, cy) = (rng.below(width), rng.below(height));
            let mut classification = classes[rng.below(classes.len())];
            let in_building = cx > width / 4 && cx < width / 2 && cy > height / 4 && cy < 3 * height / 4;
            if in_building && classification == 2 {
                classification = 6;
            }
            LidarPoint {
                x: (cx as f64 + 0.5 + (rng.next_f64() - 0.5) * 0.8) * res,
                // bbox y_max = height * res; rows count downward from y_max
                y: (height as f64 - (cy as f64 + 0.5 + (rng.next_f64() - 0.5) * 0.8)) * res,
                z: 10.0 + 20.0 * rng.next_f64(),
                classification,
            }
        })
        .collect()
}

#[test]
fn test_class_mask() {
    let all = ClassMask::from_list(None);
    assert!((0..=255u8).all(|c| all.contains(c)));
    let some = ClassMask::from_list(Some(&[2, 6, 66, 255]));
    for c in 0..=255u8 {
        assert_eq!(some.contains(c), matches!(c, 2 | 6 | 66 | 255), "class {c}");
    }
    assert!((0..=255u8).all(|c| !ClassMask::from_list(Some(&[])).contains(c)));
}

#[test]
fn test_rasterize_cpu_class_filter_and_bounds() {
    let pts = vec![
        LidarPoint { x: 0.5, y: 1.5, z: 10.0, classification: 2 },
        LidarPoint { x: 0.5, y: 1.5, z: 12.0, classification: 1 },
        LidarPoint { x: 1.5, y: 1.5, z: 8.0, classification: 2 },
        LidarPoint { x: 0.5, y: 0.5, z: 5.0, classification: 6 },
        LidarPoint { x: -0.2, y: 1.5, z: 99.0, classification: 1 }, // outside: must not land in col 0
    ];
    let (dsm, dtm) = rasterize_max_z_cpu(&pts, &ClassMask::all(), 0.0, 2.0, 1.0, 2, 2);
    assert_eq!(dsm, vec![12.0, 8.0, 5.0, f64::NEG_INFINITY]);
    assert_eq!(dtm, vec![10.0, 8.0, f64::NEG_INFINITY, f64::NEG_INFINITY]);

    let mask = ClassMask::from_list(Some(&[2]));
    let (dsm, _) = rasterize_max_z_cpu(&pts, &mask, 0.0, 2.0, 1.0, 2, 2);
    assert_eq!(dsm, vec![10.0, 8.0, f64::NEG_INFINITY, f64::NEG_INFINITY]);
}

#[test]
fn test_fill_legacy_leaves_zero_holes_nearest_does_not() {
    // 7x5 DTM: ground on the border only, 5x3 hole inside.
    let (w, h) = (7usize, 5usize);
    let dtm: Vec<f64> = (0..w * h)
        .map(|i| {
            let (r, c) = (i / w, i % w);
            if r == 0 || r == h - 1 || c == 0 || c == w - 1 {
                100.0 + i as f64
            } else {
                f64::NEG_INFINITY
            }
        })
        .collect();
    let legacy = fill_dtm_cpu(&dtm, w, h, DtmFill::Neighbor3x3);
    assert_eq!(legacy[2 * w + 3], 0.0, "centre cell has no ground neighbour in 3x3");

    let nearest = fill_dtm_cpu(&dtm, w, h, DtmFill::Nearest);
    assert!(nearest.iter().all(|&v| v >= 100.0), "no 0.0 hole with nearest fill");

    let chm = chm_cpu(&vec![120.0; w * h], &nearest);
    assert!(chm.iter().all(|&v| v < 20.0 + 1e-9));
}

#[test]
fn test_jfa_nearest_distance_exact_on_reference_grids() {
    // Checked against brute force; JFA+1 is exact on these grids.
    for (w, h) in [(40usize, 30usize), (64, 64), (97, 33)] {
        let ground: Vec<bool> = (0..w * h)
            .map(|i| {
                let (r, c) = (i / w, i % w);
                (r * 31 + c * 17) % 11 == 0
                    && !(c >= w / 4 && c < 3 * w / 4 && r >= h / 4 && r < 3 * h / 4)
            })
            .collect();
        let dtm: Vec<f64> = ground
            .iter()
            .map(|&g| if g { 1.0 } else { f64::NEG_INFINITY })
            .collect();
        let seeds = nearest_ground_seeds(&dtm, w, h);
        let d2 = |a: usize, b: usize| {
            let (ax, ay, bx, by) = ((a % w) as i64, (a / w) as i64, (b % w) as i64, (b / w) as i64);
            (ax - bx).pow(2) + (ay - by).pow(2)
        };
        for i in 0..w * h {
            let truth = (0..w * h).filter(|&j| ground[j]).map(|j| d2(i, j)).min().unwrap();
            assert_ne!(seeds[i], NO_SEED);
            assert_eq!(d2(i, seeds[i] as usize), truth, "grid {w}x{h}, cell {i}");
        }
    }
}

/// GPU (several upload batches, class mask) vs CPU, both fill modes.
#[cfg(feature = "gpu")]
#[test]
fn test_gpu_matches_cpu_fill_chm() {
    let (w, h, res) = (120usize, 80usize, 1.0);
    let bbox = (0.0, 0.0, w as f64 * res, h as f64 * res);
    let points = synthetic_points(60_000, w, h, res);

    for (mask, fill) in [
        (ClassMask::all(), DtmFill::Neighbor3x3),
        (ClassMask::from_list(Some(&[1, 2, 6, 66])), DtmFill::Neighbor3x3),
        (ClassMask::all(), DtmFill::Nearest),
        (ClassMask::from_list(Some(&[2, 6])), DtmFill::Nearest),
    ] {
        // 7 000 points per batch → 9 batches: exercises the batched upload.
        let gpu = match rasterize_fill_chm_gpu_batched(&points, bbox, &mask, res, w, h, fill, 7_000) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skip GPU parity test (no adapter / init failed): {e}");
                return;
            }
        };
        let (dsm, dtm) = rasterize_max_z_cpu(&points, &mask, bbox.0, bbox.3, res, w, h);
        let dtm = fill_dtm_cpu(&dtm, w, h, fill);
        let chm = chm_cpu(&dsm, &dtm);
        for (name, g, c) in [("dsm", &gpu.dsm, &dsm), ("dtm", &gpu.dtm, &dtm), ("chm", &gpu.chm, &chm)] {
            assert_eq!(g.len(), c.len());
            for i in 0..c.len() {
                let (gv, cv) = (g[i] as f64, c[i]);
                if cv.is_infinite() || gv.is_infinite() {
                    assert_eq!(gv, cv, "{name}[{i}] {fill:?}");
                } else {
                    assert!((gv - cv).abs() < 1e-3, "{name}[{i}] {fill:?}: gpu={gv} cpu={cv}");
                }
            }
        }
    }
}

/// Fused GPU path vs CPU (bin + fill + CHM), legacy fill.
#[cfg(feature = "gpu")]
#[test]
fn test_raster_gpu_vs_cpu_perf() {
    use super::gpu::GPU_BATCH_POINTS;
    use std::time::Instant;

    const N_POINTS: usize = 1_000_000;
    const REPEATS: usize = 3;
    const MIN_RATIO_FAIL: f64 = 0.5;

    let (w, h, res) = (1000usize, 1000usize, 1.0);
    let bbox = (0.0, 0.0, 1000.0, 1000.0);
    let points = synthetic_points(N_POINTS, w, h, res);
    let mask = ClassMask::all();
    let run_gpu = || {
        rasterize_fill_chm_gpu_batched(&points, bbox, &mask, res, w, h, DtmFill::Nearest, GPU_BATCH_POINTS)
    };
    if let Err(e) = run_gpu() {
        eprintln!("skip GPU perf gate (no adapter / init failed): {e}");
        return;
    }
    let median = |s: &mut Vec<f64>| {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    };
    let (mut cpu, mut gpu) = (Vec::new(), Vec::new());
    for _ in 0..REPEATS {
        let t = Instant::now();
        let (dsm, dtm) = rasterize_max_z_cpu(&points, &mask, bbox.0, bbox.3, res, w, h);
        let dtm = fill_dtm_cpu(&dtm, w, h, DtmFill::Nearest);
        let _ = chm_cpu(&dsm, &dtm);
        cpu.push(t.elapsed().as_secs_f64());
        let t = Instant::now();
        run_gpu().expect("GPU raster failed after successful warm-up");
        gpu.push(t.elapsed().as_secs_f64());
    }
    let (c, g) = (median(&mut cpu), median(&mut gpu));
    let ratio = if g > 0.0 { c / g } else { f64::INFINITY };
    println!("lidar raster perf (bin+JFA+CHM): gpu={g:.4}s cpu={c:.4}s ratio={ratio:.2} (n={N_POINTS})");
    assert!(ratio >= MIN_RATIO_FAIL, "GPU significantly slower than CPU: ratio={ratio:.2}");
}

/// Real-tile scale: > 16.7 M points (old dispatch limit) and > 128 MiB of points
/// (old binding limit). Slow on CPU-only machines.
#[cfg(feature = "gpu")]
#[test]
#[ignore = "large allocation; run manually with --ignored --nocapture"]
fn test_gpu_raster_large_point_count() {
    use super::gpu::rasterize_fill_chm_gpu;
    let (w, h, res) = (2000usize, 2000usize, 0.5);
    let bbox = (0.0, 0.0, w as f64 * res, h as f64 * res);
    let points = synthetic_points(20_000_000, w, h, res);
    match rasterize_fill_chm_gpu(&points, bbox, &ClassMask::all(), res, w, h, DtmFill::Nearest) {
        Ok(r) => assert_eq!(r.dsm.len(), w * h),
        Err(e) => eprintln!("GPU path refused (would fall back to CPU): {e}"),
    }
}

#[test]
fn test_cache_path_for_url() {
    let cache_dir = Path::new("/tmp/cache");
    let url = "https://example.com/path/to/file.laz";
    let path = Lidar::cache_path_for_url(cache_dir, url);
    assert_eq!(path, PathBuf::from("/tmp/cache/file.laz"));
}

#[test]
fn test_is_copc_url() {
    assert!(Lidar::is_copc_url("https://example.com/file.copc.laz"));
    assert!(Lidar::is_copc_url(
        "https://example.com/file.copc.something"
    ));
    assert!(!Lidar::is_copc_url("https://example.com/file.laz"));
    assert!(!Lidar::is_copc_url("https://example.com/file.las"));
}

#[test]
fn test_list_copc_urls() {
    let mut lidar = Lidar::new(Some("/tmp".into()), None, None).unwrap();
    assert!(lidar.list_copc_urls().is_err());

    let ign = "https://data.geopf.fr/telechargement/download/LiDARHD-NUALID/NUALHD_1-0__LAZ_LAMB93_FK_2025-02-04/LHD_FXX_0399_6580_PTS_LAMB93_IGN69.copc.laz";
    lidar.list_path_laz = Some(vec![
        ign.to_string(),
        "https://example.com/legacy.laz".to_string(),
    ]);
    let copc = lidar.list_copc_urls().unwrap();
    assert_eq!(copc, vec![ign.to_string()]);
    assert_eq!(lidar.list_laz_urls().unwrap().len(), 2);
}

#[test]
fn test_parse_wfs_tile_urls_prefers_url_npl() {
    let json = serde_json::json!({
        "features": [
            {"properties": {"url_npl": "https://example.com/a.copc.laz", "url": "https://old"}},
            {"properties": {"url": "https://example.com/legacy.laz"}},
            {"properties": {}}
        ]
    });
    assert_eq!(
        parse_wfs_tile_urls(&json),
        vec![
            "https://example.com/a.copc.laz".to_string(),
            "https://example.com/legacy.laz".to_string()
        ]
    );
}

#[test]
fn test_classification_to_u8() {
    assert_eq!(classification_to_u8(&las::point::Classification::Ground), 2);
    assert_eq!(
        classification_to_u8(&las::point::Classification::Building),
        6
    );
    assert_eq!(
        classification_to_u8(&las::point::Classification::HighVegetation),
        5
    );
    // IGN LiDAR HD: 64 sursol pérenne, 66 virtuel, 67 divers bâtis
    assert_eq!(
        classification_to_u8(&las::point::Classification::new(67).unwrap()),
        67
    );
    assert_eq!(
        classification_to_u8(&las::point::Classification::new(64).unwrap()),
        64
    );
}

#[test]
fn test_parse_copc_info() {
    let mut buf = vec![0u8; 160];
    buf[0..8].copy_from_slice(&1.0f64.to_le_bytes());
    buf[8..16].copy_from_slice(&2.0f64.to_le_bytes());
    buf[16..24].copy_from_slice(&3.0f64.to_le_bytes());
    buf[24..32].copy_from_slice(&500.0f64.to_le_bytes());
    buf[32..40].copy_from_slice(&1.0f64.to_le_bytes());
    buf[40..48].copy_from_slice(&1024u64.to_le_bytes());
    buf[48..56].copy_from_slice(&256u64.to_le_bytes());
    let info = parse_copc_info(&buf).unwrap();
    assert_eq!(info.center_x, 1.0);
    assert_eq!(info.center_y, 2.0);
    assert_eq!(info.halfsize, 500.0);
    assert_eq!(info.root_hier_offset, 1024);
    assert_eq!(info.root_hier_size, 256);
}

#[test]
fn test_copc_entry_bounds_xy() {
    let info = CopcInfo {
        center_x: 0.0,
        center_y: 0.0,
        center_z: 0.0,
        halfsize: 1000.0,
        spacing: 1.0,
        root_hier_offset: 0,
        root_hier_size: 0,
    };
    // Level 0 root (vx=0, vy=0) covers the full cube [center - halfsize, center + halfsize].
    let entry_root = CopcEntry {
        level: 0,
        vx: 0,
        vy: 0,
        vz: 0,
        offset: 0,
        byte_size: 0,
        point_count: 1,
    };
    let (min_x, min_y, max_x, max_y) = copc_entry_bounds_xy(&entry_root, &info);
    assert!((min_x - (-1000.0)).abs() < 1e-9);
    assert!((min_y - (-1000.0)).abs() < 1e-9);
    assert!((max_x - 1000.0).abs() < 1e-9, "max_x should be 1000, got {}", max_x);
    assert!((max_y - 1000.0).abs() < 1e-9);

    // Level 1, vx=1, vy=1 : side=1000, min=(0,0), max=(1000,1000)
    let entry = CopcEntry {
        level: 1,
        vx: 1,
        vy: 1,
        vz: 0,
        offset: 0,
        byte_size: 0,
        point_count: 1,
    };
    let (min_x, min_y, max_x, max_y) = copc_entry_bounds_xy(&entry, &info);
    assert!((min_x - 0.0).abs() < 1e-9);
    assert!((min_y - 0.0).abs() < 1e-9);
    assert!((max_x - 1000.0).abs() < 1e-9);
    assert!((max_y - 1000.0).abs() < 1e-9);
}

#[test]
fn test_copc_entry_bounds_xy_ign_regression() {
    // IGN LiDAR HD tile LHD_FXX_0417_6423 (Lambert-93)
    let info = CopcInfo {
        center_x: 417_500.0,
        center_y: 6_422_500.0,
        center_z: 500.0,
        halfsize: 500.0,
        spacing: 1.0,
        root_hier_offset: 0,
        root_hier_size: 0,
    };
    let bbox = (416_954.22, 6_422_714.49, 417_740.21, 6_423_250.81);

    // Northern quadrant of the tile intersects the southern portion of the query bbox.
    let entry = CopcEntry {
        level: 1,
        vx: 1,
        vy: 1,
        vz: 0,
        offset: 0,
        byte_size: 0,
        point_count: 1,
    };
    let (_nx_min, ny_min, _nx_max, ny_max) = copc_entry_bounds_xy(&entry, &info);
    assert!(ny_max > bbox.1, "node max_y {} should exceed bbox min_y {}", ny_max, bbox.1);
    assert!(ny_min < bbox.3, "node min_y {} should be below bbox max_y {}", ny_min, bbox.3);
    assert!(copc_entry_intersects(
        &entry, &info, bbox.0, bbox.1, bbox.2, bbox.3
    ));
}

#[test]
fn test_copc_entry_intersects() {
    let info = CopcInfo {
        center_x: 0.0,
        center_y: 0.0,
        center_z: 0.0,
        halfsize: 1000.0,
        spacing: 1.0,
        root_hier_offset: 0,
        root_hier_size: 0,
    };
    // Level 1, vx=1, vy=1 : side=1000, bounds (0,0)-(1000,1000)
    let entry = CopcEntry {
        level: 1,
        vx: 1,
        vy: 1,
        vz: 0,
        offset: 0,
        byte_size: 0,
        point_count: 1,
    };
    assert!(copc_entry_intersects(
        &entry, &info, 100.0, 100.0, 900.0, 900.0
    ));
    assert!(!copc_entry_intersects(
        &entry, &info, -900.0, -900.0, -100.0, -100.0
    ));
}

#[test]
fn test_parse_hierarchy_page() {
    let mut buf = vec![0u8; 32];
    buf[0..4].copy_from_slice(&1i32.to_le_bytes());
    buf[4..8].copy_from_slice(&2i32.to_le_bytes());
    buf[8..12].copy_from_slice(&3i32.to_le_bytes());
    buf[12..16].copy_from_slice(&4i32.to_le_bytes());
    buf[16..24].copy_from_slice(&1000u64.to_le_bytes());
    buf[24..28].copy_from_slice(&500i32.to_le_bytes());
    buf[28..32].copy_from_slice(&100i32.to_le_bytes());
    let entries = parse_hierarchy_page(&buf).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].level, 1);
    assert_eq!(entries[0].vx, 2);
    assert_eq!(entries[0].vy, 3);
    assert_eq!(entries[0].offset, 1000);
    assert_eq!(entries[0].byte_size, 500);
    assert_eq!(entries[0].point_count, 100);
}

#[test]
fn test_decode_las14_point() {
    let scale = [0.01, 0.01, 0.01];
    let coord_offset = [0.0, 0.0, 0.0];
    let mut raw = vec![0u8; 20];
    raw[0..4].copy_from_slice(&10000i32.to_le_bytes()); // X=100.0
    raw[4..8].copy_from_slice(&20000i32.to_le_bytes()); // Y=200.0
    raw[8..12].copy_from_slice(&5000i32.to_le_bytes()); // Z=50.0
    raw[16] = 2; // classification = Ground
    let pt = decode_las14_point(&raw, &scale, &coord_offset).unwrap();
    assert!((pt.x - 100.0).abs() < 1e-9);
    assert!((pt.y - 200.0).abs() < 1e-9);
    assert!((pt.z - 50.0).abs() < 1e-9);
    assert_eq!(pt.classification, 2);
}

#[test]
fn test_decode_las14_point_truncated() {
    let scale = [0.01, 0.01, 0.01];
    let coord_offset = [0.0, 0.0, 0.0];
    assert!(decode_las14_point(&[0u8; 16], &scale, &coord_offset).is_err());
    assert!(ru16(&[0u8; 1], 0).is_err());
}

#[test]
fn test_spatial_grid_index() {
    let points = vec![
        LidarPoint {
            x: 0.0,
            y: 0.0,
            z: 10.0,
            classification: 2,
        },
        LidarPoint {
            x: 5.0,
            y: 5.0,
            z: 15.0,
            classification: 2,
        },
        LidarPoint {
            x: 10.0,
            y: 10.0,
            z: 20.0,
            classification: 6,
        },
        LidarPoint {
            x: 15.0,
            y: 15.0,
            z: 25.0,
            classification: 6,
        },
        LidarPoint {
            x: 100.0,
            y: 100.0,
            z: 30.0,
            classification: 2,
        },
    ];
    let index = SpatialGridIndex::build_from_points(&points, 10.0);
    let candidates = index.query_bbox(0.0, 0.0, 12.0, 12.0);
    assert!(candidates.contains(&0));
    assert!(candidates.contains(&1));
    assert!(candidates.contains(&2));
    assert!(!candidates.contains(&4));
}

#[test]
fn test_quadtree_spatial_index() {
    let mut points = Vec::new();
    for i in 0..100 {
        for j in 0..100 {
            points.push(LidarPoint {
                x: i as f64 * 10.0,
                y: j as f64 * 10.0,
                z: (i + j) as f64,
                classification: 2,
            });
        }
    }
    let quadtree = QuadtreeSpatialIndex::build(&points);
    let candidates = quadtree.query_bbox(45.0, 45.0, 55.0, 55.0);
    assert!(!candidates.is_empty());
    // Coarse leaf query; exact filter matches production path.
    let exact: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|&i| {
            let p = &points[i];
            p.x >= 45.0 && p.x <= 55.0 && p.y >= 45.0 && p.y <= 55.0
        })
        .collect();
    assert!(!exact.is_empty());
    assert!(exact.len() < 100);
    assert!(exact.len() <= candidates.len());
}

#[test]
fn test_grid_cell_key() {
    let index = SpatialGridIndex::new(10.0, Some((0.0, 0.0, 100.0, 100.0)), 16);
    let key1 = index.cell_key(5.0, 5.0);
    assert_eq!(key1.col, 0);
    assert_eq!(key1.row, 0);
    let key2 = index.cell_key(15.0, 25.0);
    assert_eq!(key2.col, 1);
    assert_eq!(key2.row, 2);
    let key3 = index.cell_key(-5.0, -5.0);
    assert_eq!(key3.col, -1);
    assert_eq!(key3.row, -1);
}

#[test]
fn test_filter_points_with_spatial_index_small() {
    let points: Vec<LidarPoint> = (0..100)
        .map(|i| LidarPoint {
            x: i as f64,
            y: i as f64,
            z: i as f64,
            classification: 2,
        })
        .collect();
    let filtered = Lidar::filter_points_with_spatial_index(&points, 25.0, 25.0, 75.0, 75.0);
    assert_eq!(filtered.len(), 51);
    assert!(filtered.iter().all(|p| p.x >= 25.0 && p.x <= 75.0));
}

#[test]
fn test_filter_points_with_spatial_index_large() {
    let points: Vec<LidarPoint> = (0..50_000)
        .map(|i| LidarPoint {
            x: (i % 1000) as f64,
            y: (i / 1000) as f64 * 10.0,
            z: i as f64 * 0.1,
            classification: 2,
        })
        .collect();
    let filtered = Lidar::filter_points_with_spatial_index(&points, 100.0, 100.0, 200.0, 200.0);
    assert!(filtered
        .iter()
        .all(|p| p.x >= 100.0 && p.x <= 200.0 && p.y >= 100.0 && p.y <= 200.0));
}

#[test]
fn test_octree_node_quadrant() {
    let node = OctreeNode::new_leaf((0.0, 0.0, 0.0, 100.0, 100.0, 100.0), 0);
    assert_eq!(node.quadrant_for_point(25.0, 25.0), 0);
    assert_eq!(node.quadrant_for_point(75.0, 25.0), 1);
    assert_eq!(node.quadrant_for_point(25.0, 75.0), 2);
    assert_eq!(node.quadrant_for_point(75.0, 75.0), 3);
}

/// End-to-end stage timings for `Lidar::run` on a small IGN bbox.
///
/// Skips cleanly when WFS/cache/network is unavailable.
///
/// ```text
/// cargo test -p rsmdu --lib geometric::lidar::tests::test_lidar_run_stage_timings -- --ignored --nocapture
/// ```
#[test]
#[ignore = "network + IGN LiDAR HD; run manually with --ignored --nocapture"]
fn test_lidar_run_stage_timings() {
    let _ = crate::logging::init("rsmdu::lidar_timing=info,info");
    let output = std::env::temp_dir().join("rsmdu_lidar_e2e_timings");
    let _ = std::fs::create_dir_all(&output);

    let bbox = (-1.153414, 46.180217, -1.141098, 46.186531);
    let mut lidar = match Lidar::new(
        Some(output.to_string_lossy().into_owned()),
        None,
        None,
    ) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("skip lidar E2E timings (Lidar::new failed): {e}");
            return;
        }
    };

    if let Err(e) = lidar.set_bbox(bbox.0, bbox.1, bbox.2, bbox.3) {
        eprintln!("skip lidar E2E timings (set_bbox/WFS failed): {e}");
        return;
    }

    match lidar.run(Some("e2e_timings.tif".into()), None, Some(1.0), false) {
        Ok(path) => {
            eprintln!("lidar E2E run ok: {:?}", path);
            assert!(
                lidar
                    .loaded_points
                    .as_ref()
                    .map(|p| !p.is_empty())
                    .unwrap_or(false),
                "expected loaded points after successful run"
            );
        }
        Err(e) => {
            eprintln!("skip lidar E2E timings (run failed): {e}");
        }
    }
}

#[cfg(feature = "las")]
#[test]
fn test_save_las_ign_extended_classification() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lidar = Lidar::new(
        Some(tmp.path().to_string_lossy().into_owned()),
        None,
        None,
    )
    .unwrap();
    lidar.loaded_points = Some(vec![
        LidarPoint {
            x: 100.0,
            y: 200.0,
            z: 10.0,
            classification: 67, // IGN divers bâtis
        },
        LidarPoint {
            x: 101.0,
            y: 201.0,
            z: 11.0,
            classification: 64, // IGN sursol pérenne
        },
        LidarPoint {
            x: 102.0,
            y: 202.0,
            z: 12.0,
            classification: 6,
        },
    ]);
    let out = lidar.save_las(Path::new("ign_classes.las")).unwrap();
    assert!(out.exists());

    let mut reader = las::Reader::from_path(&out).unwrap();
    let point_data = reader.read_all().unwrap();
    let classes: Vec<u8> = point_data
        .points()
        .map(|p| classification_to_u8(&p.unwrap().classification))
        .collect();
    assert_eq!(classes, vec![67, 64, 6]);
}
