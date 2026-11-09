use std::path::{Path, PathBuf};

use super::copc::*;
use super::io::*;
use super::raster::rasterize_max_z_cpu;
use super::spatial::*;
use super::types::{classification_to_u8, Lidar, LidarPoint};

#[cfg(feature = "gpu")]
use super::gpu::rasterize_max_z_gpu;


#[cfg(feature = "gpu")]
#[test]
fn test_rasterize_max_z_gpu_matches_cpu() {
    let points = vec![
        LidarPoint {
            x: 0.5,
            y: 0.5,
            z: 10.0,
            classification: 2,
        },
        LidarPoint {
            x: 0.5,
            y: 0.5,
            z: 12.0,
            classification: 1,
        },
        LidarPoint {
            x: 1.5,
            y: 0.5,
            z: 8.0,
            classification: 2,
        },
        LidarPoint {
            x: 0.5,
            y: 1.5,
            z: 5.0,
            classification: 6,
        },
    ];
    let bbox = (0.0, 0.0, 2.0, 2.0);
    let resolution = 1.0;
    let width = 2;
    let height = 2;
    let (cpu_dsm, cpu_dtm) =
        rasterize_max_z_cpu(&points, bbox.0, bbox.3, resolution, width, height);
    let (gpu_dsm, gpu_dtm) = match rasterize_max_z_gpu(&points, bbox, resolution, width, height)
    {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skip GPU parity test (no adapter / init failed): {e}");
            return;
        }
    };
    for row in 0..height {
        for col in 0..width {
            let cd = cpu_dsm[row][col];
            let gd = gpu_dsm[row][col];
            if cd.is_finite() || gd.is_finite() {
                assert!(
                    (cd - gd).abs() < 1e-3,
                    "DSM mismatch at ({row},{col}): cpu={cd} gpu={gd}"
                );
            }
            let ct = cpu_dtm[row][col];
            let gt = gpu_dtm[row][col];
            if ct.is_finite() || gt.is_finite() {
                assert!(
                    (ct - gt).abs() < 1e-3,
                    "DTM mismatch at ({row},{col}): cpu={ct} gpu={gt}"
                );
            }
        }
    }
}

/// GPU vs CPU max-Z binning ratio (solweig-style loose floor).
#[cfg(feature = "gpu")]
#[test]
fn test_rasterize_max_z_gpu_vs_cpu_perf() {
    use std::time::Instant;

    const N_POINTS: usize = 1_000_000;
    const REPEATS: usize = 3;
    const MIN_RATIO_FAIL: f64 = 0.5;

    let bbox = (0.0_f64, 0.0, 1000.0, 1000.0);
    let resolution = 1.0_f64;
    let width = 1000;
    let height = 1000;
    let (x_min, _y_min, _x_max, y_max) = bbox;

    let points: Vec<LidarPoint> = (0..N_POINTS)
        .map(|i| {
            let t = i as f64;
            LidarPoint {
                x: (t * 37.0) % 1000.0,
                y: (t * 53.0) % 1000.0,
                z: 10.0 + (t % 50.0),
                classification: if i % 3 == 0 { 2 } else { 1 },
            }
        })
        .collect();

    // Warm-up / skip if no adapter.
    if let Err(e) = rasterize_max_z_gpu(&points, bbox, resolution, width, height) {
        eprintln!("skip GPU perf gate (no adapter / init failed): {e}");
        return;
    }

    let median = |samples: &mut [f64]| -> f64 {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        samples[samples.len() / 2]
    };

    let mut cpu_samples = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let t0 = Instant::now();
        let _ = rasterize_max_z_cpu(&points, x_min, y_max, resolution, width, height);
        cpu_samples.push(t0.elapsed().as_secs_f64());
    }

    let mut gpu_samples = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let t0 = Instant::now();
        rasterize_max_z_gpu(&points, bbox, resolution, width, height)
            .expect("GPU rasterize failed after successful warm-up");
        gpu_samples.push(t0.elapsed().as_secs_f64());
    }

    let cpu_median = median(&mut cpu_samples);
    let gpu_median = median(&mut gpu_samples);
    let ratio = if gpu_median > 0.0 {
        cpu_median / gpu_median
    } else {
        f64::INFINITY
    };

    println!(
        "lidar max-Z perf: gpu={gpu_median:.4}s cpu={cpu_median:.4}s ratio={ratio:.2} (n={N_POINTS})"
    );

    if ratio < 1.0 {
        eprintln!(
            "warning: GPU not faster than CPU (ratio={ratio:.2} < 1.0); gpu={gpu_median:.4}s cpu={cpu_median:.4}s"
        );
    }

    assert!(
        ratio >= MIN_RATIO_FAIL,
        "GPU significantly slower than CPU: gpu={gpu_median:.4}s cpu={cpu_median:.4}s ratio={ratio:.2} < {MIN_RATIO_FAIL}"
    );
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
    let entries = parse_hierarchy_page(&buf);
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
    let pt = decode_las14_point(&raw, &scale, &coord_offset);
    assert!((pt.x - 100.0).abs() < 1e-9);
    assert!((pt.y - 200.0).abs() < 1e-9);
    assert!((pt.z - 50.0).abs() < 1e-9);
    assert_eq!(pt.classification, 2);
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
    let index = SpatialGridIndex::new(10.0, Some((0.0, 0.0, 100.0, 100.0)));
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

#[cfg(all(feature = "las", feature = "tempfile"))]
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
