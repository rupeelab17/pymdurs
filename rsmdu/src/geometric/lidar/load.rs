//! Load LAZ/COPC tiles into memory.

use anyhow::{Context, Result};
use proj::Proj;
use std::path::Path;

use super::io::*;
use super::types::{classification_to_u8, Lidar, LidarPoint, TimingSpan};

#[cfg(feature = "rayon")]
use rayon::prelude::*;
#[cfg(feature = "rayon")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "progressbar")]
use indicatif::ProgressBar;
#[cfg(feature = "progressbar")]
use super::types::progress_style;

impl Lidar {


    /// Dispatcher COPC vs LAZ — used by the non-rayon load path only.
    #[cfg(not(feature = "rayon"))]
    pub(super) fn load_single_point_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        if Self::is_copc_url(url) {
            self.load_single_copc_file(url, cache_dir, filter_bbox)
        } else {
            self.load_single_laz_file(url, cache_dir, filter_bbox)
        }
    }

    // -----------------------------------------------------------------------
    // COPC NATIVE LOADER
    //
    // Remplace entièrement las::CopcEntryReader par une implémentation directe
    // de la spec COPC 1.0, en utilisant LayeredPointRecordDecompressor du crate laz.
    //
    // Flux :
    //   1. Télécharger le fichier entier (cache disque)
    //   2. Vérifier signature LASF + version LAS 1.4
    //   3. Parser COPC Info VLR (offset 429, 160 bytes)
    //   4. Trouver le LazVlr (user_id="laszip encoded", record_id=22204)
    //   5. Lire la hiérarchie d'octree depuis l'EVLR
    //   6. Filtrer les entrées qui intersectent le bbox de requête
    //   7. Pour chaque entrée : décompresser avec LayeredPointRecordDecompressor
    //      (sans chunk table — clé de la compatibilité avec IGN LiDAR HD)
    //   8. Décoder les bytes bruts en LidarPoint (LAS 1.4 Format 6)
    //   9. Si trop d'erreurs : fallback read_as_standard_laz
    // -----------------------------------------------------------------------
    pub(super) fn load_single_laz_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        use std::io::Cursor;
        let cache_path = Self::cache_path_for_url(cache_dir, url);
        let map_reader_err = |e: las::Error| {
            if cache_path.exists() {
                let _ = std::fs::remove_file(&cache_path);
            }
            anyhow::anyhow!("Failed to create LAS reader for {}: {}", url, e)
        };
        let t_io = TimingSpan::enter("download_or_cache_read");
        let mut reader = if cache_path.exists() {
            match Self::verify_cached_file(&cache_path) {
                Ok(true) => {
                    tracing::info!("Reading LAZ from cache: {:?}", cache_path);
                }
                Ok(false) | Err(_) => {
                    tracing::warn!("  Cached file appears corrupted, removing...");
                    let _ = std::fs::remove_file(&cache_path);
                }
            }
            if cache_path.exists() {
                #[cfg(feature = "laz-memmap")]
                {
                    let use_mmap = std::fs::metadata(&cache_path)
                        .map(|m| m.len() >= LAZ_MMAP_THRESHOLD_BYTES)
                        .unwrap_or(false);
                    if use_mmap {
                        let file = std::fs::File::open(&cache_path)
                            .context("Failed to open cached LAZ file")?;
                        let mmap = unsafe {
                            memmap2::Mmap::map(&file).context("Failed to mmap LAZ file")?
                        };
                        let wrapper = MmapReader { mmap, pos: 0 };
                        las::Reader::new(wrapper).map_err(map_reader_err)?
                    } else {
                        las::Reader::from_path(&cache_path).map_err(map_reader_err)?
                    }
                }
                #[cfg(not(feature = "laz-memmap"))]
                las::Reader::from_path(&cache_path).map_err(map_reader_err)?
            } else {
                tracing::info!("🌐 Downloading LAZ: {} ...", url);
                let client = crate::commons::http::download()?;
                let compressed_data = Self::download_with_verification(client, url, &cache_path)?;
                las::Reader::new(Cursor::new(compressed_data)).map_err(map_reader_err)?
            }
        } else {
            tracing::info!("🌐 Downloading LAZ: {} ...", url);
            let client = crate::commons::http::download()?;
            let compressed_data: Vec<u8> = match Self::download_laz_via_range(client, url) {
                Ok(data) => {
                    if data.len() < 4 || &data[0..4] != b"LASF" {
                        return Err(anyhow::anyhow!("Downloaded file is not a valid LAS/LAZ"));
                    }
                    std::fs::write(&cache_path, &data).context("Failed to write LAZ cache")?;
                    tracing::info!("  Cached to: {:?}", cache_path);
                    data
                }
                Err(_) => Self::download_with_verification(client, url, &cache_path)?,
            };
            las::Reader::new(Cursor::new(compressed_data)).map_err(map_reader_err)?
        };
        drop(t_io);

        let mut t_decomp = TimingSpan::enter("decompress_parse");
        let point_count = reader.header().number_of_points() as usize;
        tracing::info!("  Header declares {} points", point_count);
        let point_data = reader
            .read_all()
            .map_err(|e| anyhow::anyhow!("Failed to read LAZ points: {}", e))?;
        let mut raw_points: Vec<las::Point> = Vec::with_capacity(point_count);
        for point_result in point_data.points() {
            if let Ok(p) = point_result {
                raw_points.push(p);
            }
        }
        tracing::info!("  Read {} points", raw_points.len());

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
        t_decomp.set_points(all_points.len());
        drop(t_decomp);

        let mut t_filter = TimingSpan::enter("filter");
        let file_points = if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
            #[cfg(feature = "rayon")]
            {
                Self::filter_points_with_spatial_index_parallel(
                    &all_points,
                    x_min,
                    y_min,
                    x_max,
                    y_max,
                )
            }
            #[cfg(not(feature = "rayon"))]
            {
                Self::filter_points_with_spatial_index(&all_points, x_min, y_min, x_max, y_max)
            }
        } else {
            all_points
        };
        t_filter.set_points(file_points.len());
        tracing::info!("  Loaded {} points after spatial filter", file_points.len());
        Ok(file_points)
    }

    pub(super) fn get_lidar_points(&mut self) -> Result<(f64, f64, f64, f64, Vec<String>)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set before getting LiDAR points")?;
        tracing::info!("Bounding box set");
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;
        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;
        // WFS 2.0 default axis order is lat,lon for EPSG:4326.
        let bbox_string = format!(
            "{},{},{},{}",
            bbox.min_y, bbox.min_x, bbox.max_y, bbox.max_x
        );
        tracing::info!("🌐 Requesting LiDAR data from WFS...");
        let json = fetch_lidar_wfs_json(&bbox_string)?;
        let list_path_laz = parse_wfs_tile_urls(&json);
        tracing::info!("📍 Found {} LAZ file(s)", list_path_laz.len());
        tracing::info!("CRS: {}", self.geo_core.get_epsg());
        self.list_path_laz = Some(list_path_laz.clone());
        Ok((min_x, min_y, max_x, max_y, list_path_laz))
    }

    pub(super) fn load_lidar_points_internal(
        &self,
        laz_urls: &[String],
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        let cache_dir = self.output_path.join(".cache").join("laz");
        std::fs::create_dir_all(&cache_dir).context("Failed to create LAZ cache dir")?;

        #[cfg(feature = "rayon")]
        {
            #[cfg(feature = "progressbar")]
            let overall_pb = if laz_urls.len() > 1 {
                let pb = ProgressBar::new(laz_urls.len() as u64);
                pb.set_style(progress_style());
                pb.set_message("Files");
                pb.tick();
                Some(Arc::new(Mutex::new(pb)))
            } else {
                None
            };

            // COPC : séquentiel pour éviter le rate-limiting IGN (HTTP 429)
            // LAZ standard : parallèle comme avant
            let (copc_urls, laz_only_urls): (Vec<_>, Vec<_>) =
                laz_urls.iter().partition(|u| Self::is_copc_url(u));

            let mut all_points: Vec<LidarPoint> = Vec::new();
            let mut tile_point_counts: Vec<(String, usize)> = Vec::new();

            // --- COPC séquentiel avec retry/backoff ---
            for (idx, url) in copc_urls.iter().enumerate() {
                tracing::info!("\n📦 COPC {}/{}: {}", idx + 1, copc_urls.len(), url);
                let mut attempt = 0u32;
                let mut tile_points = 0usize;
                loop {
                    attempt += 1;
                    match self.load_single_copc_file(url, &cache_dir, filter_bbox) {
                        Ok(pts) => {
                            tile_points = pts.len();
                            tracing::info!("  ✅ {} points chargés", tile_points);
                            all_points.extend(pts);
                            break;
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            let retryable = msg.contains("429")
                                || msg.contains("Too Many Requests")
                                || msg.contains("timed out")
                                || msg.contains("connection reset");
                            if retryable && attempt < 5 {
                                let wait = 2u64.pow(attempt);
                                tracing::warn!(
                                    "  ⚠️  Erreur (tentative {}/5): {}. Retry dans {}s...",
                                    attempt, msg, wait
                                );
                                std::thread::sleep(std::time::Duration::from_secs(wait));
                            } else {
                                tracing::warn!("  ❌ Échec COPC après {} tentative(s): {}", attempt, e);
                                break;
                            }
                        }
                    }
                }
                tile_point_counts.push(((*url).clone(), tile_points));
                // Pause entre fichiers pour éviter 429
                if idx + 1 < copc_urls.len() {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                #[cfg(feature = "progressbar")]
                if let Some(ref pb) = overall_pb {
                    pb.lock().unwrap().inc(1);
                }
            }

            // --- LAZ standard : parallèle ---
            if !laz_only_urls.is_empty() {
                let results: Vec<(String, Result<Vec<LidarPoint>>)> = laz_only_urls
                    .par_iter()
                    .map(|url| {
                        let res = self.load_single_laz_file(url, &cache_dir, filter_bbox);
                        #[cfg(feature = "progressbar")]
                        if let Some(ref pb) = overall_pb {
                            pb.lock().unwrap().inc(1);
                        }
                        ((*url).clone(), res)
                    })
                    .collect();
                for (url, res) in results {
                    let pts = res?;
                    tile_point_counts.push((url, pts.len()));
                    all_points.extend(pts);
                }
            }

            Self::validate_tile_coverage(laz_urls, &tile_point_counts)?;

            #[cfg(feature = "progressbar")]
            if let Some(ref pb) = overall_pb {
                pb.lock()
                    .unwrap()
                    .finish_with_message("All files processed");
            }

            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }
            tracing::info!("Total points loaded: {}", all_points.len());
            return Ok(all_points);
        }

        #[cfg(not(feature = "rayon"))]
        {
            let mut all_points = Vec::new();
            let mut tile_point_counts: Vec<(String, usize)> = Vec::new();

            #[cfg(feature = "progressbar")]
            let overall_pb = if laz_urls.len() > 1 {
                let pb = ProgressBar::new(laz_urls.len() as u64);
                pb.set_style(progress_style());
                pb.set_message("Processing files");
                pb.tick();
                Some(pb)
            } else {
                None
            };

            for (idx, url) in laz_urls.iter().enumerate() {
                tracing::info!("\nProcessing file {}/{}: {}", idx + 1, laz_urls.len(), url);
                let mut attempt = 0u32;
                let mut tile_points = 0usize;
                loop {
                    attempt += 1;
                    match self.load_single_point_file(url, &cache_dir, filter_bbox) {
                        Ok(points) => {
                            tile_points = points.len();
                            tracing::info!("  Loaded {} points", tile_points);
                            all_points.extend(points);
                            break;
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            let retryable = Self::is_copc_url(url)
                                && (msg.contains("429")
                                    || msg.contains("Too Many Requests")
                                    || msg.contains("timed out"));
                            if retryable && attempt < 5 {
                                let wait = 2u64.pow(attempt);
                                tracing::warn!("  ⚠️  Retry {}/5 dans {}s: {}", attempt, wait, msg);
                                std::thread::sleep(std::time::Duration::from_secs(wait));
                            } else {
                                tracing::warn!("  ❌ Failed to load: {}", e);
                                break;
                            }
                        }
                    }
                }
                tile_point_counts.push(((*url).clone(), tile_points));
                #[cfg(feature = "progressbar")]
                if let Some(ref pb) = overall_pb {
                    pb.inc(1);
                    pb.tick();
                }
            }

            #[cfg(feature = "progressbar")]
            if let Some(ref pb) = overall_pb {
                pb.finish_with_message("All files processed");
            }

            Self::validate_tile_coverage(laz_urls, &tile_point_counts)?;

            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }
            tracing::info!("\nTotal points loaded: {}", all_points.len());
            Ok(all_points)
        }
    }

    /// Warn on empty tiles; fail when fewer than half of WFS tiles contributed points.
    pub(super) fn validate_tile_coverage(
        laz_urls: &[String],
        tile_point_counts: &[(String, usize)],
    ) -> Result<()> {
        let empty: Vec<&str> = tile_point_counts
            .iter()
            .filter(|(_, n)| *n == 0)
            .map(|(url, _)| url.as_str())
            .collect();
        if empty.is_empty() {
            return Ok(());
        }
        for url in &empty {
            tracing::warn!("  ⚠️  Tile returned 0 points: {}", url);
        }
        let loaded = tile_point_counts.iter().filter(|(_, n)| *n > 0).count();
        let threshold = laz_urls.len().div_ceil(2);
        if loaded < threshold {
            anyhow::bail!(
                "Only {}/{} LAZ tiles contributed points ({} empty). \
                 LiDAR coverage is incomplete.",
                loaded,
                laz_urls.len(),
                empty.len()
            );
        }
        Ok(())
    }

}
