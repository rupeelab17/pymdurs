//! LAZ/COPC download and cache helpers.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::io::*;
use super::types::Lidar;

#[cfg(feature = "progressbar")]
use super::types::progress_style;

impl Lidar {

    pub(super) fn cache_path_for_url(cache_dir: &Path, url: &str) -> PathBuf {
        let segment = url::Url::parse(url)
            .ok()
            .and_then(|u| u.path_segments().and_then(|s| s.last().map(String::from)));
        let sanitized: String = segment
            .as_deref()
            .unwrap_or("")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let filename = if sanitized.is_empty() {
            format!("unnamed_{:016x}.laz", url.len() as u64)
        } else {
            sanitized
        };
        cache_dir.join(filename)
    }

    pub(super) fn is_copc_url(url: &str) -> bool {
        url.ends_with(".copc.laz") || url.contains(".copc.")
    }

    pub(super) fn verify_cached_file(cache_path: &Path) -> Result<bool> {
        let metadata = std::fs::metadata(cache_path)?;
        if metadata.len() < LAS_HEADER_MIN_BYTES as u64 {
            return Ok(false);
        }
        let mut file = std::fs::File::open(cache_path)?;
        let mut signature = [0u8; 4];
        std::io::Read::read_exact(&mut file, &mut signature)?;
        Ok(&signature == b"LASF")
    }

    #[cfg(feature = "reqwest")]
    pub(super) fn download_with_verification(
        client: &reqwest::blocking::Client,
        url: &str,
        cache_path: &Path,
    ) -> Result<Vec<u8>> {
        use std::io::Read;
        println!("  📥 Downloading from: {}", url);
        let mut expected_size = head_content_length(client, url);
        if let Some(size) = expected_size {
            println!(
                "  Expected size: {} bytes ({:.2} MB)",
                size,
                size as f64 / 1_048_576.0
            );
        } else {
            println!("  Expected size: unknown (no Content-Length from HEAD)");
        }
        let mut retries = 3;
        let data = loop {
            let response = match client.get(url).send() {
                Ok(r) => r,
                Err(e) => {
                    retries -= 1;
                    if retries > 0 {
                        eprintln!("  Download error (retrying in 2s): {}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                    return Err(anyhow::anyhow!("Failed to download after retries: {}", e));
                }
            };
            if !response.status().is_success() {
                return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
            }
            // Prefer GET Content-Length when HEAD was missing/zero.
            if expected_size.is_none() {
                if let Some(size) = response.content_length().filter(|&n| n > 0) {
                    expected_size = Some(size);
                    println!(
                        "  Expected size (from GET): {} bytes ({:.2} MB)",
                        size,
                        size as f64 / 1_048_576.0
                    );
                }
            }
            let mut data = Vec::new();
            let mut buffer = [0u8; 65536];
            let mut response = response;
            let mut bytes_read = 0u64;
            loop {
                match response.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        data.extend_from_slice(&buffer[..n]);
                        bytes_read += n as u64;
                        if bytes_read % (10 * 1024 * 1024) < 65536 {
                            if let Some(expected) = expected_size {
                                println!(
                                    "  Progress: {:.1}% ({:.2} MB)",
                                    (bytes_read as f64 / expected as f64) * 100.0,
                                    bytes_read as f64 / 1_048_576.0
                                );
                            } else {
                                println!(
                                    "  Progress: {:.2} MB downloaded",
                                    bytes_read as f64 / 1_048_576.0
                                );
                            }
                        }
                    }
                    Err(e) => {
                        retries -= 1;
                        if retries > 0 {
                            eprintln!("  Read error (retrying): {}", e);
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            break;
                        }
                        return Err(anyhow::anyhow!("Failed to read: {}", e));
                    }
                }
            }
            if !data.is_empty() {
                if let Some(expected) = expected_size {
                    if data.len() as u64 != expected {
                        retries -= 1;
                        if retries > 0 {
                            eprintln!(
                                "  Incomplete download: got {} bytes, expected {} (retrying)",
                                data.len(),
                                expected
                            );
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            continue;
                        }
                        return Err(anyhow::anyhow!(
                            "Incomplete download: got {} bytes, expected {}",
                            data.len(),
                            expected
                        ));
                    }
                }
                break data;
            }
            retries -= 1;
            if retries == 0 {
                return Err(anyhow::anyhow!("Empty response after retries"));
            }
            eprintln!("  Empty response (retrying)");
            std::thread::sleep(std::time::Duration::from_secs(2));
        };
        if data.len() < 4 || &data[0..4] != b"LASF" {
            return Err(anyhow::anyhow!(
                "Downloaded file is not a valid LAS/LAZ file (missing LASF signature)"
            ));
        }
        println!(
            "  Downloaded {} bytes ({:.2} MB)",
            data.len(),
            data.len() as f64 / 1_048_576.0
        );
        std::fs::write(cache_path, &data).context("Failed to write cache file")?;
        println!("  Cached to: {:?}", cache_path);
        Ok(data)
    }

    #[cfg(feature = "reqwest")]
    #[allow(dead_code)]
    pub(super) fn download_laz_full_get(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut retries = 3;
        loop {
            let response = match client.get(url).send() {
                Ok(r) => r,
                Err(e) => {
                    retries -= 1;
                    if retries > 0 {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                    return Err(anyhow::anyhow!(
                        "Failed to download LAZ from {} after retries: {}",
                        url,
                        e
                    ));
                }
            };
            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "HTTP {} when downloading {}",
                    response.status(),
                    url
                ));
            }
            let mut data = Vec::new();
            let mut buffer = [0u8; 8192];
            let mut response = response;
            loop {
                match response.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => data.extend_from_slice(&buffer[..n]),
                    Err(e) => {
                        retries -= 1;
                        if retries > 0 {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            break;
                        }
                        return Err(anyhow::anyhow!("Failed to read from {}: {}", url, e));
                    }
                }
            }
            if !data.is_empty() {
                return Ok(data);
            }
            retries -= 1;
            if retries == 0 {
                return Err(anyhow::anyhow!("Empty response from {}", url));
            }
        }
    }

    #[cfg(feature = "reqwest")]
    pub(super) fn download_laz_via_range(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
        const HEADER_RANGE_END: u64 = 4095;
        let header_bytes = download_partial_laz(client, url, 0, HEADER_RANGE_END, None::<fn(u64)>)
            .map_err(|e| anyhow::anyhow!("Range request for header failed: {}", e))?;
        let parsed = parse_las_header_from_slice(&header_bytes)?;
        let offset = parsed.offset_to_point_data as u64;
        let content_length = head_content_length(client, url)
            .ok_or_else(|| anyhow::anyhow!("HEAD request failed or no Content-Length"))?;
        if offset >= content_length {
            anyhow::bail!(
                "Invalid LAZ: offset_to_point_data {} >= content_length {}",
                offset,
                content_length
            );
        }
        let header_buf: Vec<u8> = if offset <= (HEADER_RANGE_END + 1) {
            header_bytes.into_iter().take(offset as usize).collect()
        } else {
            let rest = download_partial_laz(
                client,
                url,
                HEADER_RANGE_END + 1,
                offset - 1,
                None::<fn(u64)>,
            )
            .map_err(|e| anyhow::anyhow!("Range request for header tail failed: {}", e))?;
            let mut out = header_bytes;
            out.extend(rest);
            out
        };

        #[cfg(feature = "progressbar")]
        let point_data = {
            let point_data_len = content_length - offset;
            let pb = indicatif::ProgressBar::new(point_data_len);
            pb.set_style(progress_style());
            pb.set_message("Range: point data");
            let result = download_partial_laz(
                client,
                url,
                offset,
                content_length - 1,
                Some(|n| pb.set_position(n)),
            )
            .map_err(|e| anyhow::anyhow!("Range request for point data failed: {}", e));
            pb.finish_with_message("Point data downloaded");
            result?
        };

        #[cfg(not(feature = "progressbar"))]
        let point_data =
            download_partial_laz(client, url, offset, content_length - 1, None::<fn(u64)>)
                .map_err(|e| anyhow::anyhow!("Range request for point data failed: {}", e))?;

        let mut full = header_buf;
        full.extend(point_data);
        Ok(full)
    }

}
