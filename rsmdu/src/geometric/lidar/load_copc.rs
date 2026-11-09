//! Native COPC tile loading.

use anyhow::{Context, Result};
use laz::laszip::LazVlr;
use std::path::Path;
use std::time::Instant;

use super::copc::*;
use super::types::{classification_to_u8, log_timing, Lidar, LidarPoint};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

impl Lidar {

    pub(super) fn load_single_copc_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        let cache_path = Self::cache_path_for_url(cache_dir, url);

        // --- 1. Obtenir les bytes (cache ou téléchargement) ---
        let t_io = Instant::now();
        let bytes: Vec<u8> = if cache_path.exists() {
            match Self::verify_cached_file(&cache_path) {
                Ok(true) => {
                    println!("Reading COPC from cache: {:?}", cache_path);
                    std::fs::read(&cache_path).context("Failed to read cached COPC file")?
                }
                _ => {
                    eprintln!("  Cached file appears corrupted, re-downloading...");
                    let _ = std::fs::remove_file(&cache_path);
                    let client = reqwest::blocking::Client::builder()
                        .connect_timeout(std::time::Duration::from_secs(30))
                        .timeout(std::time::Duration::from_secs(900))
                        .build()
                        .context("Failed to create HTTP client")?;
                    Self::download_with_verification(&client, url, &cache_path)?
                }
            }
        } else {
            println!("🌐 Downloading COPC: {}", url);
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .timeout(std::time::Duration::from_secs(900))
                .build()
                .context("Failed to create HTTP client")?;
            Self::download_with_verification(&client, url, &cache_path)?
        };
        log_timing("download_or_cache_read", t_io.elapsed(), None);

        println!(
            "  File size: {} bytes ({:.2} MB)",
            bytes.len(),
            bytes.len() as f64 / 1_048_576.0
        );

        // --- 2. Parser le LAS 1.4 header ---
        let hdr = match parse_las14_header(&bytes) {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "  ⚠️  Pas un fichier LAS 1.4 valide ({}), fallback LAZ standard",
                    e
                );
                return Self::read_as_standard_laz(bytes, filter_bbox);
            }
        };

        println!(
            "  LAS 1.4 | format={} record_len={} points={} vlrs={}",
            hdr.point_format, hdr.point_record_len, hdr.point_count, hdr.vlr_count
        );

        // Vérification : COPC supporte uniquement formats 6, 7, 8
        if hdr.point_format < 6 || hdr.point_format > 8 {
            eprintln!(
                "  Format de point non supporté par COPC: {} (attendu 6-8), fallback LAZ",
                hdr.point_format
            );
            return Self::read_as_standard_laz(bytes, filter_bbox);
        }

        // --- 3. Parser le COPC Info VLR (offset 429, 160 bytes) ---
        // Spec : "The info VLR MUST be the first VLR (must begin at offset 375)"
        // VLR header = 54 bytes → data commence à 375 + 54 = 429
        const COPC_INFO_DATA_OFFSET: usize = 429;
        const COPC_INFO_DATA_LEN: usize = 160;

        if bytes.len() < COPC_INFO_DATA_OFFSET + COPC_INFO_DATA_LEN {
            eprintln!("  Fichier trop court pour COPC Info VLR, fallback LAZ standard");
            return Self::read_as_standard_laz(bytes, filter_bbox);
        }

        // Vérifier que le premier VLR est bien "copc" / 1
        let first_vlr_user_id = std::str::from_utf8(&bytes[377..393])
            .unwrap_or("")
            .trim_end_matches('\0');
        let first_vlr_record_id = ru16(&bytes, 393);
        if first_vlr_user_id != "copc" || first_vlr_record_id != 1 {
            eprintln!(
                "  Premier VLR inattendu: user_id='{}' record_id={} (attendu 'copc'/1), fallback LAZ",
                first_vlr_user_id, first_vlr_record_id
            );
            return Self::read_as_standard_laz(bytes, filter_bbox);
        }

        let copc_info = match parse_copc_info(
            &bytes[COPC_INFO_DATA_OFFSET..COPC_INFO_DATA_OFFSET + COPC_INFO_DATA_LEN],
        ) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("  Échec parse COPC Info VLR: {}, fallback LAZ standard", e);
                return Self::read_as_standard_laz(bytes, filter_bbox);
            }
        };

        println!(
            "  COPC octree: centre=({:.1},{:.1},{:.1}) halfsize={:.1}",
            copc_info.center_x, copc_info.center_y, copc_info.center_z, copc_info.halfsize
        );
        println!(
            "  Hiérarchie racine: offset={} size={}",
            copc_info.root_hier_offset, copc_info.root_hier_size
        );

        // --- 4. Trouver le LazVlr ---
        let laz_vlr = match find_laz_vlr(&bytes, hdr.vlr_count) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  LazVlr introuvable: {}, fallback LAZ standard", e);
                return Self::read_as_standard_laz(bytes, filter_bbox);
            }
        };

        // --- 5. Lire toutes les entrées de la hiérarchie ---
        let all_entries = match read_all_hierarchy_entries(&bytes, &copc_info) {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "  Échec lecture hiérarchie COPC: {}, fallback LAZ standard",
                    e
                );
                return Self::read_as_standard_laz(bytes, filter_bbox);
            }
        };

        println!("  Hiérarchie COPC: {} entrées totales", all_entries.len());

        // --- 6. Filtrer les entrées qui intersectent le bbox ---
        let entries_to_process: Vec<&CopcEntry> =
            if let Some((qx_min, qy_min, qx_max, qy_max)) = filter_bbox {
                println!(
                    "  Spatial filter: [{:.2}, {:.2}] -> [{:.2}, {:.2}]",
                    qx_min, qy_min, qx_max, qy_max
                );
                all_entries
                    .iter()
                    .filter(|e| {
                        e.point_count > 0
                            && copc_entry_intersects(e, &copc_info, qx_min, qy_min, qx_max, qy_max)
                    })
                    .collect()
            } else {
                all_entries.iter().filter(|e| e.point_count > 0).collect()
            };

        println!(
            "  {} entries à décompresser (sur {})",
            entries_to_process.len(),
            all_entries.len()
        );

        let leaf_entry_count = all_entries.iter().filter(|e| e.point_count > 0).count();
        if entries_to_process.is_empty() && leaf_entry_count > 0 {
            eprintln!(
                "  ⚠️  COPC spatial filter matched 0 chunks ({} leaf entries in hierarchy), falling back to full LAZ read",
                leaf_entry_count
            );
            return Self::read_as_standard_laz(bytes, filter_bbox);
        }

        // --- 7 & 8. Décompresser chaque chunk et décoder les points ---
        let t_decomp = Instant::now();
        let stats =
            Self::decompress_copc_entries(&bytes, &entries_to_process, &laz_vlr, &hdr, filter_bbox);
        log_timing(
            "decompress_parse",
            t_decomp.elapsed(),
            Some(stats.points.len()),
        );

        println!("  COPC Results:");
        println!("     - Entries processed: {}", stats.entries_processed);
        println!("     - Successfully read: {}", stats.entries_success);
        if stats.entries_failed > 0 {
            let failure_rate = stats.entries_failed as f64 / stats.entries_processed.max(1) as f64;
            println!(
                "     - Failed to read: {} ({:.1}%)",
                stats.entries_failed,
                failure_rate * 100.0
            );
        }
        println!("     - Points loaded: {}", stats.points.len());

        // --- 9. Fallback si taux d'échec trop élevé ou aucun point ---
        let failure_rate = stats.entries_failed as f64 / stats.entries_processed.max(1) as f64;
        if (failure_rate > 0.5 && stats.entries_processed > 0) || stats.points.is_empty() {
            if stats.points.is_empty() && stats.entries_processed > 0 {
                eprintln!(
                    "  ⚠️  COPC decompression returned 0 points, falling back to full LAZ read"
                );
            } else if failure_rate > 0.5 {
                eprintln!(
                    "  High failure rate ({:.1}%), falling back to standard LAZ reader (using cached file)",
                    failure_rate * 100.0
                );
            }
            return Self::read_as_standard_laz(bytes, filter_bbox);
        }

        Ok(stats.points)
    }

    /// Décompresse une entry COPC ; retourne (points, success).
    fn decompress_one_copc_entry(
        file_buf: &[u8],
        entry: &CopcEntry,
        laz_vlr: &LazVlr,
        hdr: &Las14Header,
        filter_bbox: Option<(f64, f64, f64, f64)>,
        idx: usize,
    ) -> (Vec<LidarPoint>, bool) {
        let chunk_start = entry.offset as usize;
        let chunk_end = chunk_start + entry.byte_size as usize;
        if chunk_end > file_buf.len() {
            eprintln!(
                "  ⚠️  Chunk {} hors limites (offset={} size={} file_len={})",
                idx,
                entry.offset,
                entry.byte_size,
                file_buf.len()
            );
            return (Vec::new(), false);
        }
        let chunk_buf = &file_buf[chunk_start..chunk_end];
        match decompress_copc_chunk(chunk_buf, laz_vlr, hdr.point_record_len, entry.point_count) {
            Ok(raw_points) => {
                let point_size = hdr.point_record_len as usize;
                let n = entry.point_count as usize;
                let mut points = Vec::with_capacity(n);
                for i in 0..n {
                    let raw = &raw_points[i * point_size..(i + 1) * point_size];
                    let pt = decode_las14_point(raw, &hdr.scale, &hdr.coord_offset);
                    if let Some((qx_min, qy_min, qx_max, qy_max)) = filter_bbox {
                        if pt.x < qx_min || pt.x > qx_max || pt.y < qy_min || pt.y > qy_max {
                            continue;
                        }
                    }
                    points.push(pt);
                }
                (points, true)
            }
            Err(e) => {
                if idx < 10 {
                    eprintln!(
                        "  ⚠️  Chunk {} (level={} vx={} vy={}): {}",
                        idx, entry.level, entry.vx, entry.vy, e
                    );
                }
                (Vec::new(), false)
            }
        }
    }

    /// Décompresse les entries COPC sélectionnées (parallèle si feature `rayon`).
    pub(super) fn decompress_copc_entries(
        file_buf: &[u8],
        entries: &[&CopcEntry],
        laz_vlr: &LazVlr,
        hdr: &Las14Header,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> CopcReadStats {
        #[cfg(feature = "rayon")]
        {
            let results: Vec<(Vec<LidarPoint>, bool)> = entries
                .par_iter()
                .enumerate()
                .map(|(idx, entry)| {
                    Self::decompress_one_copc_entry(
                        file_buf, entry, laz_vlr, hdr, filter_bbox, idx,
                    )
                })
                .collect();

            let entries_processed = results.len();
            let mut entries_success = 0usize;
            let mut entries_failed = 0usize;
            let mut result_points = Vec::new();
            for (pts, ok) in results {
                if ok {
                    entries_success += 1;
                    result_points.extend(pts);
                } else {
                    entries_failed += 1;
                }
            }
            return CopcReadStats {
                points: result_points,
                entries_processed,
                entries_success,
                entries_failed,
            };
        }

        #[cfg(not(feature = "rayon"))]
        {
            let mut result_points: Vec<LidarPoint> = Vec::new();
            let mut entries_processed = 0usize;
            let mut entries_success = 0usize;
            let mut entries_failed = 0usize;

            for (idx, entry) in entries.iter().enumerate() {
                entries_processed += 1;
                let (pts, ok) = Self::decompress_one_copc_entry(
                    file_buf, entry, laz_vlr, hdr, filter_bbox, idx,
                );
                if ok {
                    entries_success += 1;
                    result_points.extend(pts);
                } else {
                    entries_failed += 1;
                }
                if (idx + 1) % 500 == 0 {
                    println!(
                        "  Progress: {}/{} entries, {} points",
                        idx + 1,
                        entries.len(),
                        result_points.len()
                    );
                }
            }

            CopcReadStats {
                points: result_points,
                entries_processed,
                entries_success,
                entries_failed,
            }
        }
    }

    pub(super) fn read_as_standard_laz(
        bytes: Vec<u8>,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        use std::io::Cursor;
        println!("  📖 Reading as standard LAZ file...");
        let t_decomp = Instant::now();
        let cursor = Cursor::new(bytes);
        let mut reader = las::Reader::new(cursor)
            .map_err(|e| anyhow::anyhow!("Failed to create LAZ reader: {}", e))?;
        let point_count = reader.header().number_of_points();
        println!("  Header declares {} points", point_count);
        let point_data = reader
            .read_all()
            .map_err(|e| anyhow::anyhow!("Failed to read LAZ points: {}", e))?;
        let mut raw_points: Vec<las::Point> = Vec::with_capacity(point_count as usize);
        let mut errors = 0;
        for point_result in point_data.points() {
            match point_result {
                Ok(p) => raw_points.push(p),
                Err(_) => {
                    errors += 1;
                }
            }
        }
        if errors > 0 {
            eprintln!("  {} point read errors", errors);
        }
        println!("  Read {} points from LAZ", raw_points.len());

        #[cfg(feature = "rayon")]
        let all_points: Vec<LidarPoint> = raw_points
            .par_iter()
            .map(|p| LidarPoint {
                x: p.x,
                y: p.y,
                z: p.z,
                classification: classification_to_u8(&p.classification),
            })
            .collect();

        #[cfg(not(feature = "rayon"))]
        let all_points: Vec<LidarPoint> = raw_points
            .iter()
            .map(|p| LidarPoint {
                x: p.x,
                y: p.y,
                z: p.z,
                classification: classification_to_u8(&p.classification),
            })
            .collect();
        log_timing(
            "decompress_parse",
            t_decomp.elapsed(),
            Some(all_points.len()),
        );

        let t_filter = Instant::now();
        let file_points = if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
            Self::filter_points_with_spatial_index(&all_points, x_min, y_min, x_max, y_max)
        } else {
            all_points
        };
        log_timing("filter", t_filter.elapsed(), Some(file_points.len()));
        println!("  Loaded {} points after spatial filter", file_points.len());
        Ok(file_points)
    }

}
