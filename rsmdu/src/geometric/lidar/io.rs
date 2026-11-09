//! LiDAR download, mmap, WFS helpers.

use anyhow::{Context, Result};

pub(super) const LAS_HEADER_MIN_BYTES: usize = 111;
pub(super) const LAS_OFFSET_TO_POINT_DATA: usize = 94;
pub(super) const LAS_NUMBER_OF_POINT_RECORDS: usize = 107;

#[derive(Debug)]
pub(super) struct LasHeaderParsed {
    pub(super) offset_to_point_data: u32,
    #[allow(dead_code)]
    pub(super) number_of_points: u64,
}

pub(super) fn parse_las_header_from_slice(buf: &[u8]) -> Result<LasHeaderParsed> {
    if buf.len() < LAS_HEADER_MIN_BYTES {
        anyhow::bail!(
            "LAS header buffer too short: need at least {} bytes, got {}",
            LAS_HEADER_MIN_BYTES,
            buf.len()
        );
    }
    if buf.get(0..4) != Some(b"LASF") {
        anyhow::bail!("Invalid LAS signature (expected LASF)");
    }
    let offset_to_point_data = u32::from_le_bytes(
        buf[LAS_OFFSET_TO_POINT_DATA..LAS_OFFSET_TO_POINT_DATA + 4]
            .try_into()
            .unwrap(),
    );
    let number_of_points = u32::from_le_bytes(
        buf[LAS_NUMBER_OF_POINT_RECORDS..LAS_NUMBER_OF_POINT_RECORDS + 4]
            .try_into()
            .unwrap(),
    ) as u64;
    Ok(LasHeaderParsed {
        offset_to_point_data,
        number_of_points,
    })
}

#[cfg(feature = "laz-memmap")]
pub(super) struct MmapReader {
    pub(super) mmap: memmap2::Mmap,
    pub(super) pos: u64,
}

#[cfg(feature = "laz-memmap")]
impl std::io::Read for MmapReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let slice = self.mmap.as_ref();
        let start = self.pos as usize;
        if start >= slice.len() {
            return Ok(0);
        }
        let n = std::cmp::min(buf.len(), slice.len() - start);
        buf[..n].copy_from_slice(&slice[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

#[cfg(feature = "laz-memmap")]
impl std::io::Seek for MmapReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let len = self.mmap.len() as u64;
        self.pos = match pos {
            std::io::SeekFrom::Start(n) => n,
            std::io::SeekFrom::End(n) => {
                if n >= 0 {
                    len.saturating_add(n as u64)
                } else {
                    len.saturating_sub((-n) as u64)
                }
            }
            std::io::SeekFrom::Current(n) => {
                if n >= 0 {
                    self.pos.saturating_add(n as u64)
                } else {
                    self.pos.saturating_sub((-n) as u64)
                }
            }
        };
        Ok(self.pos)
    }
}

#[cfg(feature = "laz-memmap")]
pub(super) const LAZ_MMAP_THRESHOLD_BYTES: u64 = 50 * 1024 * 1024;

#[cfg(feature = "reqwest")]
pub(super) fn download_partial_laz<F>(
    client: &reqwest::blocking::Client,
    url: &str,
    start: u64,
    end: u64,
    mut on_progress: Option<F>,
) -> Result<Vec<u8>>
where
    F: FnMut(u64),
{
    use std::io::Read;
    let response = client
        .get(url)
        .header("Range", format!("bytes={}-{}", start, end))
        .send()
        .context("Range request failed")?;
    let status = response.status();
    if status == reqwest::StatusCode::OK {
        return Err(anyhow::anyhow!(
            "Server returned 200 (Range not supported), use full GET"
        ));
    }
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(anyhow::anyhow!(
            "Expected 206 Partial Content, got {}",
            status
        ));
    }
    let mut data = Vec::new();
    let mut response = response;
    let mut buffer = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let n = response.read(&mut buffer).context("Read Range body")?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..n]);
        total += n as u64;
        if let Some(ref mut f) = on_progress {
            f(total);
        }
    }
    Ok(data)
}

#[cfg(feature = "reqwest")]
pub(super) fn head_content_length(client: &reqwest::blocking::Client, url: &str) -> Option<u64> {
    let response = client.head(url).send().ok()?;
    // Some CDNs (e.g. data.geopf.fr) answer HEAD with Content-Length: 0.
    // Treat 0 / missing as unknown so callers don't divide by zero or retry forever.
    response.content_length().filter(|&n| n > 0)
}

/// Public Géoplateforme WFS. Layer `IGNF_NUAGES-DE-POINTS-LIDAR-HD:dalle` was removed
/// (HTTP 400 XML `Unknown namespace`); tile index is now metadata with `url_npl`.
pub(super) const LIDAR_WFS_URL: &str = "https://data.geopf.fr/wfs/ows";
pub(super) const LIDAR_WFS_TYPENAME: &str = "IGNF_LIDAR-HD_METADONNEE:metadata";

pub(super) fn fetch_lidar_wfs_json(bbox: &str) -> Result<serde_json::Value> {
    let params = [
        ("service", "WFS"),
        ("version", "2.0.0"),
        ("request", "GetFeature"),
        ("typeNames", LIDAR_WFS_TYPENAME),
        ("outputFormat", "application/json"),
        ("bbox", bbox),
    ];
    let response = reqwest::blocking::Client::new()
        .get(LIDAR_WFS_URL)
        .query(&params)
        .header("Accept", "application/json")
        .header("User-Agent", "pymdurs/0.1")
        .send()
        .with_context(|| format!("Failed to send WFS request to {LIDAR_WFS_URL}"))?;
    let status = response.status();
    let text = response
        .text()
        .context("Failed to read WFS response body")?;
    if !status.is_success() {
        anyhow::bail!(
            "WFS request failed ({}): {}",
            status,
            truncate_for_error(&text, 400)
        );
    }
    serde_json::from_str(&text).with_context(|| {
        format!(
            "Failed to parse WFS JSON response: {}",
            truncate_for_error(&text, 300)
        )
    })
}

pub(super) fn truncate_for_error(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(max_chars).collect::<String>())
    }
}

pub(super) fn lidar_tile_url_from_feature(feature: &serde_json::Value) -> Option<String> {
    let props = feature.get("properties")?;
    props
        .get("url_npl")
        .or_else(|| props.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string)
}

pub(super) fn parse_wfs_tile_urls(json: &serde_json::Value) -> Vec<String> {
    json.get("features")
        .and_then(|f| f.as_array())
        .into_iter()
        .flatten()
        .filter_map(lidar_tile_url_from_feature)
        .collect()
}
