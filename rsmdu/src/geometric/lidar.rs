use anyhow::{Context, Result};
use proj::Proj;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "rayon")]
use rayon::prelude::*;
#[cfg(feature = "rayon")]
use std::sync::{Arc, Mutex};

use crate::geo_core::{BoundingBox, GeoCore};

#[cfg(feature = "indicatif")]
use indicatif::{ProgressBar, ProgressStyle};

// ============================================================================
// SPATIAL INDEXING
// ============================================================================

/// Cell key for spatial grid indexing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GridCellKey {
    col: i64,
    row: i64,
}

/// Spatial grid index for fast point queries
/// Uses a regular grid to bucket points by location
#[derive(Debug)]
struct SpatialGridIndex {
    /// Cell size in coordinate units (e.g., meters)
    cell_size: f64,
    /// Origin X coordinate
    origin_x: f64,
    /// Origin Y coordinate  
    origin_y: f64,
    /// Grid cells containing point indices
    cells: HashMap<GridCellKey, Vec<usize>>,
    /// Total number of indexed points
    point_count: usize,
}

impl SpatialGridIndex {
    /// Create a new spatial grid index
    ///
    /// # Arguments
    /// * `cell_size` - Size of each grid cell (larger = fewer cells, more points per cell)
    /// * `bounds` - Optional (min_x, min_y, max_x, max_y) to set origin
    fn new(cell_size: f64, bounds: Option<(f64, f64, f64, f64)>) -> Self {
        let (origin_x, origin_y) = bounds
            .map(|(min_x, min_y, _, _)| (min_x, min_y))
            .unwrap_or((0.0, 0.0));

        SpatialGridIndex {
            cell_size,
            origin_x,
            origin_y,
            cells: HashMap::new(),
            point_count: 0,
        }
    }

    /// Get the grid cell key for a point
    #[inline]
    fn cell_key(&self, x: f64, y: f64) -> GridCellKey {
        GridCellKey {
            col: ((x - self.origin_x) / self.cell_size).floor() as i64,
            row: ((y - self.origin_y) / self.cell_size).floor() as i64,
        }
    }

    /// Build index from a slice of points
    fn build_from_points(points: &[LidarPoint], cell_size: f64) -> Self {
        // First pass: find bounds
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for p in points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }

        let mut index = SpatialGridIndex::new(cell_size, Some((min_x, min_y, max_x, max_y)));

        // Second pass: insert points
        for (i, p) in points.iter().enumerate() {
            let key = index.cell_key(p.x, p.y);
            index.cells.entry(key).or_insert_with(Vec::new).push(i);
        }

        index.point_count = points.len();
        index
    }

    /// Query points within a bounding box
    /// Returns indices of points that MAY be within the bbox (need final filtering)
    fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let min_col = ((min_x - self.origin_x) / self.cell_size).floor() as i64;
        let max_col = ((max_x - self.origin_x) / self.cell_size).floor() as i64;
        let min_row = ((min_y - self.origin_y) / self.cell_size).floor() as i64;
        let max_row = ((max_y - self.origin_y) / self.cell_size).floor() as i64;

        let mut result = Vec::new();

        for col in min_col..=max_col {
            for row in min_row..=max_row {
                let key = GridCellKey { col, row };
                if let Some(indices) = self.cells.get(&key) {
                    result.extend(indices);
                }
            }
        }

        result
    }

    /// Get statistics about the index
    fn stats(&self) -> SpatialIndexStats {
        let cell_count = self.cells.len();
        let total_points = self.point_count;
        let avg_points_per_cell = if cell_count > 0 {
            total_points as f64 / cell_count as f64
        } else {
            0.0
        };
        let max_points_in_cell = self.cells.values().map(|v| v.len()).max().unwrap_or(0);

        SpatialIndexStats {
            cell_count,
            total_points,
            avg_points_per_cell,
            max_points_in_cell,
            cell_size: self.cell_size,
        }
    }
}

/// Statistics about a spatial index
#[derive(Debug)]
struct SpatialIndexStats {
    cell_count: usize,
    total_points: usize,
    avg_points_per_cell: f64,
    max_points_in_cell: usize,
    cell_size: f64,
}

impl std::fmt::Display for SpatialIndexStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SpatialIndex: {} cells, {} points, {:.1} avg/cell, {} max/cell, {:.1}m cell size",
            self.cell_count,
            self.total_points,
            self.avg_points_per_cell,
            self.max_points_in_cell,
            self.cell_size
        )
    }
}

/// Octree node for hierarchical spatial indexing (similar to COPC structure)
#[derive(Debug)]
struct OctreeNode {
    /// Bounding box of this node
    bounds: (f64, f64, f64, f64, f64, f64), // min_x, min_y, min_z, max_x, max_y, max_z
    /// Point indices stored in this node (leaf nodes only)
    points: Vec<usize>,
    /// Child nodes (8 children for 3D octree, but we use 4 for 2D quadtree)
    children: Option<Box<[Option<OctreeNode>; 4]>>,
    /// Depth of this node
    depth: u8,
}

impl OctreeNode {
    /// Maximum points per leaf node before splitting
    const MAX_POINTS_PER_NODE: usize = 1000;
    /// Maximum depth to prevent infinite recursion
    const MAX_DEPTH: u8 = 12;

    /// Create a new leaf node
    fn new_leaf(bounds: (f64, f64, f64, f64, f64, f64), depth: u8) -> Self {
        OctreeNode {
            bounds,
            points: Vec::new(),
            children: None,
            depth,
        }
    }

    /// Check if a point is within this node's XY bounds
    #[inline]
    fn contains_xy(&self, x: f64, y: f64) -> bool {
        x >= self.bounds.0 && x <= self.bounds.3 && y >= self.bounds.1 && y <= self.bounds.4
    }

    /// Check if this node's bounds intersect with a query bbox
    #[inline]
    fn intersects_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        !(self.bounds.3 < min_x
            || self.bounds.0 > max_x
            || self.bounds.4 < min_y
            || self.bounds.1 > max_y)
    }

    /// Get the quadrant index for a point (0-3)
    fn quadrant_for_point(&self, x: f64, y: f64) -> usize {
        let mid_x = (self.bounds.0 + self.bounds.3) / 2.0;
        let mid_y = (self.bounds.1 + self.bounds.4) / 2.0;

        let right = x >= mid_x;
        let top = y >= mid_y;

        match (right, top) {
            (false, false) => 0, // SW
            (true, false) => 1,  // SE
            (false, true) => 2,  // NW
            (true, true) => 3,   // NE
        }
    }

    /// Get bounds for a child quadrant
    fn child_bounds(&self, quadrant: usize) -> (f64, f64, f64, f64, f64, f64) {
        let mid_x = (self.bounds.0 + self.bounds.3) / 2.0;
        let mid_y = (self.bounds.1 + self.bounds.4) / 2.0;

        match quadrant {
            0 => (
                self.bounds.0,
                self.bounds.1,
                self.bounds.2,
                mid_x,
                mid_y,
                self.bounds.5,
            ), // SW
            1 => (
                mid_x,
                self.bounds.1,
                self.bounds.2,
                self.bounds.3,
                mid_y,
                self.bounds.5,
            ), // SE
            2 => (
                self.bounds.0,
                mid_y,
                self.bounds.2,
                mid_x,
                self.bounds.4,
                self.bounds.5,
            ), // NW
            3 => (
                mid_x,
                mid_y,
                self.bounds.2,
                self.bounds.3,
                self.bounds.4,
                self.bounds.5,
            ), // NE
            _ => unreachable!(),
        }
    }

    /// Insert a point index into the tree
    fn insert(&mut self, point_idx: usize, points: &[LidarPoint]) {
        let point = &points[point_idx];

        if !self.contains_xy(point.x, point.y) {
            return;
        }

        // If we have children, delegate to the appropriate child
        if self.children.is_some() {
            // Calculate quadrant and bounds BEFORE borrowing children mutably
            let quadrant = self.quadrant_for_point(point.x, point.y);
            let child_bounds = self.child_bounds(quadrant);
            let depth = self.depth;

            let children = self.children.as_mut().unwrap();
            if children[quadrant].is_none() {
                children[quadrant] = Some(OctreeNode::new_leaf(child_bounds, depth + 1));
            }
            if let Some(ref mut child) = children[quadrant] {
                child.insert(point_idx, points);
            }
            return;
        }

        // Add to this leaf node
        self.points.push(point_idx);

        // Check if we need to split
        if self.points.len() > Self::MAX_POINTS_PER_NODE && self.depth < Self::MAX_DEPTH {
            self.split(points);
        }
    }

    /// Split this node into 4 children
    fn split(&mut self, points: &[LidarPoint]) {
        // Create children array
        self.children = Some(Box::new([None, None, None, None]));

        // Move points to children
        let old_points = std::mem::take(&mut self.points);
        for point_idx in old_points {
            self.insert(point_idx, points);
        }
    }

    /// Query points within a bounding box
    fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, result: &mut Vec<usize>) {
        if !self.intersects_bbox(min_x, min_y, max_x, max_y) {
            return;
        }

        // Add points from this node
        result.extend(&self.points);

        // Recurse into children
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                child.query_bbox(min_x, min_y, max_x, max_y, result);
            }
        }
    }

    /// Count total points in this subtree
    fn count_points(&self) -> usize {
        let mut count = self.points.len();
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                count += child.count_points();
            }
        }
        count
    }

    /// Count nodes in this subtree
    fn count_nodes(&self) -> usize {
        let mut count = 1;
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                count += child.count_nodes();
            }
        }
        count
    }
}

/// Quadtree-based spatial index for LiDAR points
#[derive(Debug)]
pub struct QuadtreeSpatialIndex {
    root: OctreeNode,
}

impl QuadtreeSpatialIndex {
    /// Build a quadtree index from points
    pub(crate) fn build(points: &[LidarPoint]) -> Self {
        // Find bounds
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;

        for p in points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            min_z = min_z.min(p.z);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
            max_z = max_z.max(p.z);
        }

        // Add small padding to ensure all points fit
        let padding = 0.001;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;

        let mut root = OctreeNode::new_leaf((min_x, min_y, min_z, max_x, max_y, max_z), 0);

        // Insert all points
        for i in 0..points.len() {
            root.insert(i, points);
        }

        QuadtreeSpatialIndex { root }
    }

    /// Query points within a bounding box
    pub fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let mut result = Vec::new();
        self.root
            .query_bbox(min_x, min_y, max_x, max_y, &mut result);
        result
    }

    /// Get statistics
    pub fn stats(&self) -> String {
        format!(
            "Quadtree: {} nodes, {} points indexed",
            self.root.count_nodes(),
            self.root.count_points()
        )
    }
}

// ============================================================================
// LIDAR POINT AND MAIN STRUCTURES
// ============================================================================

#[cfg(feature = "indicatif")]
fn progress_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {percent} {msg}")
        .unwrap()
        .progress_chars("##-")
}

/// Lidar structure
/// Following Python implementation from pymdu.image.Lidar
/// Provides methods to collect and process LiDAR point cloud data
pub struct Lidar {
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Output path for processed data
    output_path: PathBuf,
    /// Classification filter (optional)
    classification: Option<u8>,
    /// List of LAZ file URLs (populated by get_lidar_points)
    list_path_laz: Option<Vec<String>>,
    /// Loaded LiDAR points (populated by load_lidar_points)
    loaded_points: Option<Vec<LidarPoint>>,
}

/// Point structure for LiDAR data
#[derive(Debug, Clone)]
pub(crate) struct LidarPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) classification: u8,
}

/// Minimum bytes needed to parse LAS public header (offset to point data at 94-97, number of points at 107-110).
const LAS_HEADER_MIN_BYTES: usize = 111;

/// LAS public header: offset to point data at bytes 94-97 (u32 LE).
const LAS_OFFSET_TO_POINT_DATA: usize = 94;
/// LAS public header: number of point records at bytes 107-110 (u32 LE) for LAS 1.0-1.2.
const LAS_NUMBER_OF_POINT_RECORDS: usize = 107;

/// Parsed LAS/LAZ header from a partial buffer (e.g. first 4KB from Range request).
#[derive(Debug)]
struct LasHeaderParsed {
    /// Byte offset from start of file where point data begins.
    offset_to_point_data: u32,
    /// Total number of point records (for LAS 1.0-1.2; 4-byte field). Used for logging/progress.
    #[allow(dead_code)]
    number_of_points: u64,
}

/// Parse LAS/LAZ public header from a buffer (at least 111 bytes).
/// Returns offset to point data and number of point records for use with HTTP Range.
fn parse_las_header_from_slice(buf: &[u8]) -> Result<LasHeaderParsed> {
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

/// Wrapper around memory-mapped file that implements Read + Seek for las::Reader.
/// Used for large cached LAZ files when feature "laz-memmap" is enabled.
#[cfg(feature = "laz-memmap")]
struct MmapReader {
    mmap: memmap2::Mmap,
    pos: u64,
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

/// Threshold above which to use memory-mapped I/O for cached LAZ (50 MiB).
#[cfg(feature = "laz-memmap")]
const LAZ_MMAP_THRESHOLD_BYTES: u64 = 50 * 1024 * 1024;

/// Download a byte range of a LAZ file via HTTP Range request (blocking).
/// Returns `Ok(data)` only when server responds with 206 Partial Content.
/// Returns `Err` on 200 (Range not supported) or other failure so caller can fall back to full GET.
/// If `on_progress` is provided, it is called with the number of bytes read so far (cumulative).
#[cfg(feature = "reqwest")]
fn download_partial_laz<F>(
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
        // Server ignored Range and may have sent full body; caller should use full GET instead
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

/// Fetch Content-Length via HEAD request. Returns None if HEAD fails or header is missing.
#[cfg(feature = "reqwest")]
fn head_content_length(client: &reqwest::blocking::Client, url: &str) -> Option<u64> {
    let response = client.head(url).send().ok()?;
    response.content_length()
}

/// Map las classification enum to u8 (for parallel conversion).
#[inline]
fn classification_to_u8(c: &las::point::Classification) -> u8 {
    match c {
        las::point::Classification::CreatedNeverClassified => 0,
        las::point::Classification::Unclassified => 1,
        las::point::Classification::Ground => 2,
        las::point::Classification::LowVegetation => 3,
        las::point::Classification::MediumVegetation => 4,
        las::point::Classification::HighVegetation => 5,
        las::point::Classification::Building => 6,
        las::point::Classification::LowPoint => 7,
        las::point::Classification::ModelKeyPoint => 8,
        las::point::Classification::Water => 9,
        _ => 1,
    }
}

/// Processed raster data
struct ProcessedRasters {
    dsm: Vec<Vec<f64>>, // Digital Surface Model
    dtm: Vec<Vec<f64>>, // Digital Terrain Model
    chm: Vec<Vec<f64>>, // Canopy Height Model
    width: usize,
    height: usize,
    transform: [f64; 6], // GDAL-style transform
}

/// Result of COPC entry reading for statistics
#[cfg(feature = "lidar-copc")]
struct CopcReadResult {
    points: Vec<LidarPoint>,
    entries_processed: usize,
    entries_success: usize,
    entries_failed: usize,
}

impl Lidar {
    /// Create a new Lidar instance
    /// Following Python: def __init__(self, output_path=None, classification=None)
    /// If bbox is provided, get_lidar_points() is called immediately
    pub fn new(
        output_path: Option<String>,
        classification: Option<u8>,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Self> {
        use crate::collect::global_variables::TEMP_PATH;

        let output_path_buf = PathBuf::from(
            output_path
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or(TEMP_PATH),
        );

        let mut lidar = Lidar {
            geo_core: GeoCore::default(), // Default to EPSG:2154 (Lambert-93)
            output_path: output_path_buf,
            classification,
            list_path_laz: None,
            loaded_points: None,
        };

        // If bbox is provided, set it and get LiDAR points immediately
        if let Some((min_x, min_y, max_x, max_y)) = bbox {
            lidar.set_bbox(min_x, min_y, max_x, max_y)?;
        }

        Ok(lidar)
    }

    /// Set bounding box and get LiDAR points URLs, then load the points
    /// Following Python: lidar.bbox = [min_x, min_y, max_x, max_y]
    /// This also calls get_lidar_points() and load_lidar_points() to fetch and load LAZ files
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<()> {
        self.geo_core
            .set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));

        // Get LiDAR points URLs and bbox in EPSG:2154 immediately when bbox is set
        let (min_x, min_y, max_x, max_y, _) = self.get_lidar_points()?;

        // Load LiDAR points from URLs with early spatial filter
        if let Some(ref laz_urls) = self.list_path_laz {
            if !laz_urls.is_empty() {
                let filter_bbox = Some((min_x, min_y, max_x, max_y));
                let points = self.load_lidar_points_internal(laz_urls, filter_bbox)?;
                self.loaded_points = Some(points);
            }
        }

        Ok(())
    }

    /// Set classification filter
    /// Following Python: lidar.classification = value
    pub fn set_classification(&mut self, classification: Option<u8>) {
        self.classification = classification;
    }

    /// Get output path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }

    /// Compute cache file path for a LAZ URL. Uses last path segment, sanitized for filesystem.
    fn cache_path_for_url(cache_dir: &Path, url: &str) -> PathBuf {
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

    /// Returns true if the URL is for a COPC file (Cloud Optimized Point Cloud).
    fn is_copc_url(url: &str) -> bool {
        url.ends_with(".copc.laz") || url.contains(".copc.")
    }

    /// Verify that a cached file is valid (has correct LAS signature and reasonable size)
    fn verify_cached_file(cache_path: &Path) -> Result<bool> {
        let metadata = std::fs::metadata(cache_path)?;

        // File should be at least large enough for a LAS header
        if metadata.len() < LAS_HEADER_MIN_BYTES as u64 {
            return Ok(false);
        }

        // Check LAS signature
        let mut file = std::fs::File::open(cache_path)?;
        let mut signature = [0u8; 4];
        std::io::Read::read_exact(&mut file, &mut signature)?;

        Ok(&signature == b"LASF")
    }

    /// Download a file with integrity verification
    #[cfg(feature = "reqwest")]
    fn download_with_verification(
        client: &reqwest::blocking::Client,
        url: &str,
        cache_path: &Path,
    ) -> Result<Vec<u8>> {
        use std::io::Read;

        println!("  📥 Downloading from: {}", url);

        // Get expected size first via HEAD request
        let expected_size = head_content_length(client, url);
        if let Some(size) = expected_size {
            println!(
                "  Expected size: {} bytes ({:.2} MB)",
                size,
                size as f64 / 1_048_576.0
            );
        }

        // Download with retries
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

            let mut data = Vec::new();
            let mut buffer = [0u8; 65536]; // 64KB buffer for faster downloads
            let mut response = response;
            let mut bytes_read = 0u64;

            loop {
                match response.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        data.extend_from_slice(&buffer[..n]);
                        bytes_read += n as u64;

                        // Progress every 10MB
                        if bytes_read % (10 * 1024 * 1024) < 65536 {
                            if let Some(expected) = expected_size {
                                println!(
                                    "  Progress: {:.1}%",
                                    (bytes_read as f64 / expected as f64) * 100.0
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
                // Verify size if we know expected
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

        // Verify LAS signature before caching
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

        // Cache the file
        std::fs::write(cache_path, &data).context("Failed to write cache file")?;
        println!("  Cached to: {:?}", cache_path);

        Ok(data)
    }

    /// Load a single point file (COPC or LAZ) from URL or cache. Dispatches to COPC or LAZ loader.
    fn load_single_point_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        if !Self::is_copc_url(url) {
            return self.load_single_laz_file(url, cache_dir, filter_bbox);
        }
        #[cfg(feature = "lidar-copc")]
        return self.load_single_copc_file(url, cache_dir, filter_bbox);
        #[cfg(not(feature = "lidar-copc"))]
        {
            eprintln!(
                "COPC URL detected but lidar-copc feature disabled; loading as LAZ: {}",
                url
            );
            self.load_single_laz_file(url, cache_dir, filter_bbox)
        }
    }

    /// Read points from a byte buffer as a standard LAZ file
    /// This is the fallback method when COPC reading fails
    /// Uses spatial indexing for efficient bbox filtering
    fn read_as_standard_laz(
        bytes: Vec<u8>,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        use std::io::Cursor;

        println!("  📖 Reading as standard LAZ file...");

        let cursor = Cursor::new(bytes);
        let mut reader = las::Reader::new(cursor)
            .map_err(|e| anyhow::anyhow!("Failed to create LAZ reader: {}", e))?;

        let point_count = reader.header().number_of_points();
        println!("  Header declares {} points", point_count);

        // Read all points first
        let mut raw_points: Vec<las::Point> = Vec::with_capacity(point_count as usize);
        let mut errors = 0;

        for point_result in reader.points() {
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

        // Convert to LidarPoint first (needed for spatial indexing)
        #[cfg(feature = "rayon")]
        let all_points: Vec<LidarPoint> = raw_points
            .par_iter()
            .map(|point| LidarPoint {
                x: point.x,
                y: point.y,
                z: point.z,
                classification: classification_to_u8(&point.classification),
            })
            .collect();

        #[cfg(not(feature = "rayon"))]
        let all_points: Vec<LidarPoint> = raw_points
            .iter()
            .map(|point| LidarPoint {
                x: point.x,
                y: point.y,
                z: point.z,
                classification: classification_to_u8(&point.classification),
            })
            .collect();

        // Apply spatial filtering using index if we have a bbox
        let file_points = if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
            Self::filter_points_with_spatial_index(&all_points, x_min, y_min, x_max, y_max)
        } else {
            all_points
        };

        println!("  Loaded {} points after spatial filter", file_points.len());

        Ok(file_points)
    }

    /// Filter points using spatial indexing for better performance on large datasets
    /// Chooses between grid index (faster to build) and quadtree (faster queries) based on data size
    fn filter_points_with_spatial_index(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();

        // For small datasets, just do linear scan
        if point_count < 10_000 {
            return points
                .iter()
                .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
                .cloned()
                .collect();
        }

        println!("  Building spatial index for {} points...", point_count);
        let start = std::time::Instant::now();

        // Choose index type based on expected selectivity
        // Grid index is faster to build, quadtree is better for very selective queries
        let query_area = (x_max - x_min) * (y_max - y_min);

        // Estimate data bounds from sample
        let sample_size = (point_count / 100).max(100).min(point_count);
        let step = point_count / sample_size;
        let mut data_min_x = f64::INFINITY;
        let mut data_min_y = f64::INFINITY;
        let mut data_max_x = f64::NEG_INFINITY;
        let mut data_max_y = f64::NEG_INFINITY;

        for i in (0..point_count).step_by(step.max(1)) {
            let p = &points[i];
            data_min_x = data_min_x.min(p.x);
            data_min_y = data_min_y.min(p.y);
            data_max_x = data_max_x.max(p.x);
            data_max_y = data_max_y.max(p.y);
        }

        let data_area = (data_max_x - data_min_x) * (data_max_y - data_min_y);
        let selectivity = if data_area > 0.0 {
            query_area / data_area
        } else {
            1.0
        };

        println!("  📐 Query selectivity: {:.1}%", selectivity * 100.0);

        // Use grid index for moderate selectivity, quadtree for very selective queries
        let result = if selectivity > 0.5 || point_count < 100_000 {
            // Grid index - faster to build
            // Cell size based on expected point density
            let cell_size = ((data_max_x - data_min_x) / 100.0)
                .max((data_max_y - data_min_y) / 100.0)
                .max(10.0); // Minimum 10m cells

            let grid_index = SpatialGridIndex::build_from_points(points, cell_size);
            println!("  {}", grid_index.stats());

            let candidate_indices = grid_index.query_bbox(x_min, y_min, x_max, y_max);
            println!(
                "  🔍 Grid query returned {} candidates",
                candidate_indices.len()
            );

            // Final precise filtering
            candidate_indices
                .into_iter()
                .filter_map(|i| {
                    let p = &points[i];
                    if p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max {
                        Some(p.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            // Quadtree - better for very selective queries on large datasets
            let quadtree = QuadtreeSpatialIndex::build(points);
            println!("  {}", quadtree.stats());

            let candidate_indices = quadtree.query_bbox(x_min, y_min, x_max, y_max);
            println!(
                "  🔍 Quadtree query returned {} candidates",
                candidate_indices.len()
            );

            // Final precise filtering
            candidate_indices
                .into_iter()
                .filter_map(|i| {
                    let p = &points[i];
                    if p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max {
                        Some(p.clone())
                    } else {
                        None
                    }
                })
                .collect()
        };

        let elapsed = start.elapsed();
        println!(
            "  Spatial indexing and query took {:.2}s",
            elapsed.as_secs_f64()
        );

        result
    }

    /// Filter points using parallel spatial indexing (for very large datasets)
    #[cfg(feature = "rayon")]
    fn filter_points_with_spatial_index_parallel(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();

        // For datasets over 1M points, use chunked parallel processing
        if point_count < 1_000_000 {
            return Self::filter_points_with_spatial_index(points, x_min, y_min, x_max, y_max);
        }

        println!(
            "  🚀 Using parallel spatial indexing for {} points...",
            point_count
        );
        let start = std::time::Instant::now();

        // Split into chunks and process in parallel
        let chunk_size = 500_000;
        let chunks: Vec<_> = points.chunks(chunk_size).collect();

        let results: Vec<Vec<LidarPoint>> = chunks
            .par_iter()
            .map(|chunk| Self::filter_points_with_spatial_index(chunk, x_min, y_min, x_max, y_max))
            .collect();

        let result: Vec<LidarPoint> = results.into_iter().flatten().collect();

        let elapsed = start.elapsed();
        println!(
            "  Parallel spatial filtering took {:.2}s",
            elapsed.as_secs_f64()
        );

        result
    }

    /// Load a single COPC file with proper error handling and fallback to standard LAZ
    #[cfg(feature = "lidar-copc")]
    fn load_single_copc_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        use std::io::Cursor;

        let cache_path = Self::cache_path_for_url(cache_dir, url);

        // Try to load from cache or download
        let bytes: Vec<u8> = if cache_path.exists() {
            // Verify cached file integrity
            match Self::verify_cached_file(&cache_path) {
                Ok(true) => {
                    println!("Reading COPC from cache: {:?}", cache_path);
                    std::fs::read(&cache_path).context("Failed to read cached file")?
                }
                Ok(false) | Err(_) => {
                    eprintln!("  Cached file appears corrupted, re-downloading...");
                    let _ = std::fs::remove_file(&cache_path);

                    let client = reqwest::blocking::Client::builder()
                        .connect_timeout(std::time::Duration::from_secs(30))
                        .timeout(std::time::Duration::from_secs(900)) // 15 min timeout for large files
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

        println!(
            "  File size: {} bytes ({:.2} MB)",
            bytes.len(),
            bytes.len() as f64 / 1_048_576.0
        );

        // Try COPC reader first
        let cursor = Cursor::new(bytes.clone());
        match las::CopcEntryReader::new(cursor) {
            Ok(mut entry_reader) => {
                // Check for COPC info VLR
                if entry_reader.header().copc_info_vlr().is_none() {
                    println!("  File missing COPC VLR, falling back to standard LAZ reader");
                    return Self::read_as_standard_laz(bytes, filter_bbox);
                }

                // Get hierarchy entries
                let entries = match entry_reader.hierarchy_entries() {
                    Some(e) => e,
                    None => {
                        println!(
                            "  Could not read COPC hierarchy, falling back to standard LAZ reader"
                        );
                        return Self::read_as_standard_laz(bytes, filter_bbox);
                    }
                };

                println!("  COPC hierarchy: {} entries", entries.len());

                if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
                    println!(
                        "  Spatial filter: [{:.2}, {:.2}] -> [{:.2}, {:.2}]",
                        x_min, y_min, x_max, y_max
                    );
                }

                // Try to read entries
                let result = Self::read_copc_entries(&mut entry_reader, &entries, filter_bbox);

                // Check if we had too many failures
                let failure_rate = if result.entries_processed > 0 {
                    result.entries_failed as f64 / result.entries_processed as f64
                } else {
                    0.0
                };

                println!("  COPC Results:");
                println!("     - Entries processed: {}", result.entries_processed);
                println!("     - Successfully read: {}", result.entries_success);
                if result.entries_failed > 0 {
                    println!(
                        "     - Failed to read: {} ({:.1}%)",
                        result.entries_failed,
                        failure_rate * 100.0
                    );
                }
                println!("     - Points loaded: {}", result.points.len());

                // If more than 50% failures, fall back to standard LAZ on the same cached bytes (no re-download)
                if failure_rate > 0.5 {
                    eprintln!(
                        "  High failure rate ({:.1}%), falling back to standard LAZ reader (using cached file)",
                        failure_rate * 100.0
                    );
                    return Self::read_as_standard_laz(bytes, filter_bbox);
                }

                // If we got no points but had successful reads, the bbox might be outside the data
                if result.points.is_empty() && result.entries_success > 0 {
                    println!("  No points found in bbox (data may be outside the query area)");
                }

                Ok(result.points)
            }
            Err(e) => {
                eprintln!("  COPC reader failed: {}", e);
                eprintln!("  📖 Falling back to standard LAZ reader");
                Self::read_as_standard_laz(bytes, filter_bbox)
            }
        }
    }

    /// Read COPC entries and return statistics
    #[cfg(feature = "lidar-copc")]
    fn read_copc_entries<R: std::io::Read + std::io::Seek>(
        entry_reader: &mut las::CopcEntryReader<R>,
        entries: &[las::copc::Entry],
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> CopcReadResult {
        let mut all_points: Vec<las::Point> = Vec::new();
        let mut chunk = Vec::new();

        let mut entries_processed = 0;
        let mut entries_success = 0;
        let mut entries_failed = 0;

        for entry in entries {
            if entry.point_count <= 0 {
                continue;
            }

            entries_processed += 1;
            chunk.clear();

            match entry_reader.read_entry_points(entry, &mut chunk) {
                Ok(_) => {
                    entries_success += 1;

                    // Apply spatial filter if provided
                    if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
                        let filtered: Vec<las::Point> = chunk
                            .drain(..)
                            .filter(|p| {
                                p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max
                            })
                            .collect();
                        all_points.extend(filtered);
                    } else {
                        all_points.extend(chunk.drain(..));
                    }
                }
                Err(_) => {
                    entries_failed += 1;
                }
            }

            // Progress every 500 entries
            if entries_processed % 500 == 0 {
                println!(
                    "  Progress: {}/{} entries, {} points",
                    entries_processed,
                    entries.len(),
                    all_points.len()
                );
            }
        }

        // Convert to LidarPoint
        #[cfg(feature = "rayon")]
        let points: Vec<LidarPoint> = all_points
            .par_iter()
            .map(|p| LidarPoint {
                x: p.x,
                y: p.y,
                z: p.z,
                classification: classification_to_u8(&p.classification),
            })
            .collect();

        #[cfg(not(feature = "rayon"))]
        let points: Vec<LidarPoint> = all_points
            .iter()
            .map(|p| LidarPoint {
                x: p.x,
                y: p.y,
                z: p.z,
                classification: classification_to_u8(&p.classification),
            })
            .collect();

        CopcReadResult {
            points,
            entries_processed,
            entries_success,
            entries_failed,
        }
    }

    /// Download full LAZ file with a single GET (fallback when Range is not supported).
    #[cfg(feature = "reqwest")]
    #[allow(dead_code)]
    fn download_laz_full_get(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
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

    /// Download LAZ via HTTP Range: header first, then point data. Returns full file bytes.
    /// Fails with Err if server does not support Range (206) or HEAD Content-Length.
    #[cfg(feature = "reqwest")]
    fn download_laz_via_range(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
        const HEADER_RANGE_END: u64 = 4095; // bytes 0..4096

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

        #[cfg(feature = "indicatif")]
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

        #[cfg(not(feature = "indicatif"))]
        let point_data =
            download_partial_laz(client, url, offset, content_length - 1, None::<fn(u64)>)
                .map_err(|e| anyhow::anyhow!("Range request for point data failed: {}", e))?;

        let mut full = header_buf;
        full.extend(point_data);
        Ok(full)
    }

    /// Load a single LAZ file from URL or cache; used for parallel and sequential loading.
    /// Creates its own HTTP client when downloading (safe for use from multiple threads).
    fn load_single_laz_file(
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

        let mut reader = if cache_path.exists() {
            // Verify cached file first
            match Self::verify_cached_file(&cache_path) {
                Ok(true) => {
                    println!("Reading LAZ from cache: {:?}", cache_path);
                }
                Ok(false) | Err(_) => {
                    eprintln!("  Cached file appears corrupted, removing...");
                    let _ = std::fs::remove_file(&cache_path);
                    // Fall through to download
                }
            }

            if cache_path.exists() {
                #[cfg(feature = "laz-memmap")]
                let use_mmap = std::fs::metadata(&cache_path)
                    .map(|m| m.len() >= LAZ_MMAP_THRESHOLD_BYTES)
                    .unwrap_or(false);
                #[cfg(feature = "laz-memmap")]
                if use_mmap {
                    let file = std::fs::File::open(&cache_path)
                        .context("Failed to open cached LAZ file")?;
                    let mmap =
                        unsafe { memmap2::Mmap::map(&file).context("Failed to mmap LAZ file")? };
                    let wrapper = MmapReader { mmap, pos: 0 };
                    las::Reader::new(wrapper).map_err(map_reader_err)?
                } else {
                    las::Reader::from_path(&cache_path).map_err(map_reader_err)?
                }
                #[cfg(not(feature = "laz-memmap"))]
                las::Reader::from_path(&cache_path).map_err(map_reader_err)?
            } else {
                // File was removed, need to download
                println!("🌐 Downloading LAZ: {} ...", url);
                let client = reqwest::blocking::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(30))
                    .timeout(std::time::Duration::from_secs(600))
                    .build()
                    .context("Failed to create HTTP client")?;

                let compressed_data = Self::download_with_verification(&client, url, &cache_path)?;
                las::Reader::new(Cursor::new(compressed_data)).map_err(map_reader_err)?
            }
        } else {
            println!("🌐 Downloading LAZ: {} ...", url);
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .context("Failed to create HTTP client")?;

            let compressed_data: Vec<u8> = match Self::download_laz_via_range(&client, url) {
                Ok(data) => {
                    // Verify and cache
                    if data.len() < 4 || &data[0..4] != b"LASF" {
                        return Err(anyhow::anyhow!("Downloaded file is not a valid LAS/LAZ"));
                    }
                    std::fs::write(&cache_path, &data).context("Failed to write LAZ cache")?;
                    println!("  Cached to: {:?}", cache_path);
                    data
                }
                Err(_) => {
                    // Fallback: full GET (Range not supported or HEAD/parse failed)
                    Self::download_with_verification(&client, url, &cache_path)?
                }
            };

            las::Reader::new(Cursor::new(compressed_data)).map_err(map_reader_err)?
        };

        let point_count = reader.header().number_of_points() as usize;
        println!("  Header declares {} points", point_count);

        let mut raw_points: Vec<las::Point> = Vec::with_capacity(point_count);
        for point_result in reader.points() {
            if let Ok(p) = point_result {
                raw_points.push(p);
            }
        }

        println!("  Read {} points", raw_points.len());

        // Convert to LidarPoint
        #[cfg(feature = "rayon")]
        let all_points: Vec<LidarPoint> = raw_points
            .par_iter()
            .map(|point| LidarPoint {
                x: point.x,
                y: point.y,
                z: point.z,
                classification: classification_to_u8(&point.classification),
            })
            .collect();

        #[cfg(not(feature = "rayon"))]
        let all_points: Vec<LidarPoint> = raw_points
            .iter()
            .map(|point| LidarPoint {
                x: point.x,
                y: point.y,
                z: point.z,
                classification: classification_to_u8(&point.classification),
            })
            .collect();

        // Apply spatial filtering using index if we have a bbox
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

        println!("  Loaded {} points after spatial filter", file_points.len());

        Ok(file_points)
    }

    /// Get LiDAR point cloud URLs from WFS service
    /// Following Python: def _get_lidar_points(self)
    /// Returns transformed bbox and list of LAZ file URLs
    fn get_lidar_points(&mut self) -> Result<(f64, f64, f64, f64, Vec<String>)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set before getting LiDAR points")?;

        println!("Bounding box set");

        // Transform bbox from EPSG:4326 to EPSG:2154
        // Python: transformer = Transformer.from_crs("EPSG:4326", "EPSG:2154", always_xy=True)
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;

        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;

        // Create bbox string for WFS request
        let bbox_string = format!(
            "{},{},{},{}",
            bbox.min_y, bbox.min_x, bbox.max_y, bbox.max_x
        );

        let url = "https://data.geopf.fr/wfs/ows";
        let params = [
            ("service", "WFS"),
            ("version", "2.0.0"),
            ("request", "GetFeature"),
            ("typeName", "IGNF_NUAGES-DE-POINTS-LIDAR-HD:dalle"),
            ("outputFormat", "application/json"),
            ("bbox", &bbox_string),
        ];

        println!("🌐 Requesting LiDAR data from WFS...");

        let response = reqwest::blocking::Client::new()
            .get(url)
            .query(&params)
            .header("Accept", "application/json")
            .send()
            .context("Failed to send WFS request")?;

        let json: serde_json::Value = response
            .json()
            .context("Failed to parse WFS JSON response")?;

        // Extract URLs from features
        let mut list_path_laz = Vec::new();
        if let Some(features) = json
            .get("features")
            .and_then(|f: &serde_json::Value| f.as_array())
        {
            for feature in features {
                if let Some(url) = feature
                    .get("properties")
                    .and_then(|p: &serde_json::Value| p.get("url"))
                    .and_then(|u: &serde_json::Value| u.as_str())
                {
                    list_path_laz.push(url.to_string());
                }
            }
        }

        println!("📍 Found {} LAZ file(s)", list_path_laz.len());
        println!("CRS: {}", self.geo_core.get_epsg());

        // Store the URLs
        self.list_path_laz = Some(list_path_laz.clone());

        Ok((min_x, min_y, max_x, max_y, list_path_laz))
    }

    /// Download and load LiDAR points from LAZ URLs (internal method)
    /// Following Python: def load_lidar_points(self, laz_urls)
    /// Returns vector of LidarPoint with (x, y, z, classification)
    /// Uses las crate with laz-parallel feature for LAZ decompression.
    /// If filter_bbox is Some((x_min, y_min, x_max, y_max)) in EPSG:2154, points outside are skipped during conversion.
    fn load_lidar_points_internal(
        &self,
        laz_urls: &[String],
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        let cache_dir = self.output_path.join(".cache").join("laz");
        std::fs::create_dir_all(&cache_dir).context("Failed to create LAZ cache dir")?;

        #[cfg(feature = "rayon")]
        {
            #[cfg(feature = "indicatif")]
            let overall_pb = if laz_urls.len() > 1 {
                let pb = ProgressBar::new(laz_urls.len() as u64);
                pb.set_style(progress_style());
                pb.set_message("Files");
                pb.tick();
                Some(Arc::new(Mutex::new(pb)))
            } else {
                None
            };

            let results: Vec<Result<Vec<LidarPoint>>> = laz_urls
                .par_iter()
                .map(|url| {
                    let res = self.load_single_point_file(url, &cache_dir, filter_bbox);
                    #[cfg(feature = "indicatif")]
                    if let Some(ref pb) = overall_pb {
                        pb.lock().unwrap().inc(1);
                    }
                    res
                })
                .collect();

            #[cfg(feature = "indicatif")]
            if let Some(ref pb) = overall_pb {
                pb.lock()
                    .unwrap()
                    .finish_with_message("All files processed");
            }

            let vecs: Vec<Vec<LidarPoint>> = results.into_iter().collect::<Result<Vec<_>, _>>()?;
            let all_points: Vec<LidarPoint> = vecs.into_iter().flatten().collect();
            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }
            println!("Total points loaded: {}", all_points.len());
            return Ok(all_points);
        }

        #[cfg(not(feature = "rayon"))]
        {
            let mut all_points = Vec::new();

            #[cfg(feature = "indicatif")]
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
                println!("\nProcessing file {}/{}: {}", idx + 1, laz_urls.len(), url);

                match self.load_single_point_file(url, &cache_dir, filter_bbox) {
                    Ok(points) => {
                        println!("  Loaded {} points", points.len());
                        all_points.extend(points);
                    }
                    Err(e) => {
                        eprintln!("  ❌ Failed to load: {}", e);
                        // Continue with other files
                    }
                }

                #[cfg(feature = "indicatif")]
                if let Some(ref pb) = overall_pb {
                    pb.inc(1);
                    pb.tick();
                }
            }

            #[cfg(feature = "indicatif")]
            if let Some(ref pb) = overall_pb {
                pb.finish_with_message("All files processed");
            }

            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }

            println!("\nTotal points loaded: {}", all_points.len());
            Ok(all_points)
        }
    }

    /// Process LiDAR points to create DSM, DTM, and CHM rasters
    /// Following Python: def process_lidar_points(self, points, bbox, classification_list, resolution)
    /// Returns ProcessedRasters with DSM, DTM, and CHM grids
    fn process_lidar_points(
        &self,
        points: Vec<LidarPoint>,
        bbox: (f64, f64, f64, f64),
        classification_list: Option<Vec<u8>>,
        resolution: f64,
    ) -> Result<ProcessedRasters> {
        let (x_min, y_min, x_max, y_max) = bbox;

        // Filter points by spatial bbox
        let filtered_points: Vec<LidarPoint> = points
            .into_iter()
            .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
            .collect();

        println!("Filtered {} points within bbox", filtered_points.len());

        // Apply classification filter if provided
        let filtered_points: Vec<LidarPoint> = if let Some(ref class_list) = classification_list {
            filtered_points
                .into_iter()
                .filter(|p| class_list.contains(&p.classification))
                .collect()
        } else {
            filtered_points
        };

        println!(
            "After classification filter: {} points",
            filtered_points.len()
        );

        // Calculate grid dimensions
        let width = ((x_max - x_min) / resolution).ceil() as usize;
        let height = ((y_max - y_min) / resolution).ceil() as usize;

        println!(
            "Grid dimensions: {}x{} (resolution: {}m)",
            width, height, resolution
        );

        // Initialize grids
        let mut dsm = vec![vec![f64::NEG_INFINITY; width]; height];
        let mut dtm = vec![vec![f64::NEG_INFINITY; width]; height];

        // Process points to fill grids
        for point in &filtered_points {
            // Calculate grid indices
            let col = ((point.x - x_min) / resolution).floor() as usize;
            let row = ((y_max - point.y) / resolution).floor() as usize; // Y is inverted in raster

            if col < width && row < height {
                // DSM: maximum z value per cell (all points)
                if point.z > dsm[row][col] || dsm[row][col] == f64::NEG_INFINITY {
                    dsm[row][col] = point.z;
                }

                // DTM: maximum z value per cell for ground points only (classification 2)
                if point.classification == 2 {
                    if point.z > dtm[row][col] || dtm[row][col] == f64::NEG_INFINITY {
                        dtm[row][col] = point.z;
                    }
                }
            }
        }

        // Fill DTM gaps using interpolation (simple: use nearest neighbor)
        // For now, we'll use a simple approach: if a cell has no ground point, use the minimum of neighbors
        let mut dtm_filled = dtm.clone();
        for row in 0..height {
            for col in 0..width {
                if dtm_filled[row][col] == f64::NEG_INFINITY {
                    // Find minimum value from neighbors
                    let mut min_neighbor = f64::INFINITY;
                    for dr in [-1, 0, 1] {
                        for dc in [-1, 0, 1] {
                            let r = row as i32 + dr;
                            let c = col as i32 + dc;
                            if r >= 0 && r < height as i32 && c >= 0 && c < width as i32 {
                                let val = dtm[r as usize][c as usize];
                                if val != f64::NEG_INFINITY && val < min_neighbor {
                                    min_neighbor = val;
                                }
                            }
                        }
                    }
                    if min_neighbor != f64::INFINITY {
                        dtm_filled[row][col] = min_neighbor;
                    } else {
                        dtm_filled[row][col] = 0.0; // Fallback
                    }
                }
            }
        }

        // Calculate CHM = DSM - DTM
        let mut chm = vec![vec![0.0; width]; height];
        for row in 0..height {
            for col in 0..width {
                if dsm[row][col] != f64::NEG_INFINITY && dtm_filled[row][col] != f64::NEG_INFINITY {
                    chm[row][col] = dsm[row][col] - dtm_filled[row][col];
                    // Ensure non-negative
                    if chm[row][col] < 0.0 {
                        chm[row][col] = 0.0;
                    }
                }
            }
        }

        // Create GDAL-style transform
        // [x_origin, pixel_width, 0, y_origin, 0, -pixel_height]
        let transform = [
            x_min,       // x_origin
            resolution,  // pixel_width
            0.0,         // rotation (not used)
            y_max,       // y_origin
            0.0,         // rotation (not used)
            -resolution, // pixel_height (negative because Y increases downward)
        ];

        Ok(ProcessedRasters {
            dsm,
            dtm: dtm_filled,
            chm,
            width,
            height,
            transform,
        })
    }

    /// Convert processed rasters to GeoTIFF file
    /// Following Python: def to_tif(self, write_out_file, classification_list)
    /// Creates a multi-band GeoTIFF with DSM, DTM, and CHM
    fn to_tif(
        &self,
        rasters: &ProcessedRasters,
        output_path: &Path,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        use gdal::raster::Buffer;
        use gdal::spatial_ref::SpatialRef;

        // Create output directory if needed
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        if write_out_file {
            // Get GTiff driver
            let driver = gdal::DriverManager::get_driver_by_name("GTiff")
                .context("Failed to get GTiff driver")?;

            // Create dataset
            let mut dataset = driver
                .create_with_band_type::<f64, _>(
                    output_path,
                    rasters.width,
                    rasters.height,
                    3, // 3 bands: DSM, DTM, CHM
                )
                .context("Failed to create GeoTIFF dataset")?;

            // Set geotransform
            dataset
                .set_geo_transform(&rasters.transform)
                .context("Failed to set geotransform")?;

            // Set spatial reference (EPSG:2154)
            let srs = SpatialRef::from_epsg(self.geo_core.get_epsg() as u32)
                .context("Failed to create spatial reference")?;
            dataset
                .set_spatial_ref(&srs)
                .context("Failed to set spatial reference")?;

            // Write bands
            // Band 1: DSM
            {
                let mut band = dataset.rasterband(1).context("Failed to get band 1")?;

                // Convert 2D vec to flat array (row-major)
                let mut data = Vec::with_capacity(rasters.width * rasters.height);
                for row in &rasters.dsm {
                    for &val in row {
                        data.push(if val == f64::NEG_INFINITY {
                            f64::NAN
                        } else {
                            val
                        });
                    }
                }

                let mut buffer = Buffer::new((rasters.width, rasters.height), data);
                band.write((0, 0), (rasters.width, rasters.height), &mut buffer)
                    .context("Failed to write DSM band")?;
                band.set_no_data_value(Some(f64::NAN))
                    .context("Failed to set no data value for DSM")?;
            }

            // Band 2: DTM
            {
                let mut band = dataset.rasterband(2).context("Failed to get band 2")?;

                let mut data = Vec::with_capacity(rasters.width * rasters.height);
                for row in &rasters.dtm {
                    for &val in row {
                        data.push(if val == f64::NEG_INFINITY {
                            f64::NAN
                        } else {
                            val
                        });
                    }
                }

                let mut buffer = Buffer::new((rasters.width, rasters.height), data);
                band.write((0, 0), (rasters.width, rasters.height), &mut buffer)
                    .context("Failed to write DTM band")?;
                band.set_no_data_value(Some(f64::NAN))
                    .context("Failed to set no data value for DTM")?;
            }

            // Band 3: CHM
            {
                let mut band = dataset.rasterband(3).context("Failed to get band 3")?;

                let mut data = Vec::with_capacity(rasters.width * rasters.height);
                for row in &rasters.chm {
                    for &val in row {
                        data.push(val);
                    }
                }

                let mut buffer = Buffer::new((rasters.width, rasters.height), data);
                band.write((0, 0), (rasters.width, rasters.height), &mut buffer)
                    .context("Failed to write CHM band")?;
                band.set_no_data_value(Some(0.0))
                    .context("Failed to set no data value for CHM")?;
            }

            println!("GeoTIFF saved to: {:?}", output_path);
        }

        Ok(output_path.to_path_buf())
    }

    /// Run the complete LiDAR processing workflow
    /// Following Python workflow: load points → process → create GeoTIFF
    /// Note: get_lidar_points() is now called in set_bbox(), so URLs are already available
    /// Returns path to the created GeoTIFF file
    pub fn run(
        &mut self,
        file_name: Option<String>,
        classification_list: Option<Vec<u8>>,
        resolution: Option<f64>,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        let resolution = resolution.unwrap_or(1.0);

        // Get LAZ file URLs (already fetched in set_bbox)
        let laz_urls = self
            .list_path_laz
            .as_ref()
            .context("No LAZ URLs available. Call set_bbox() first.")?;

        if laz_urls.is_empty() {
            anyhow::bail!("No LAZ files found for the specified bounding box");
        }

        // Get bbox for processing (already transformed in get_lidar_points)
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set")?;

        // Transform bbox from EPSG:4326 to EPSG:2154 (same as in get_lidar_points)
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;

        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;

        // Use already loaded points (loaded in set_bbox)
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded. Call set_bbox() first.")?
            .clone();

        // Process points to create rasters
        let rasters = self.process_lidar_points(
            points,
            (min_x, min_y, max_x, max_y),
            classification_list,
            resolution,
        )?;

        // Create GeoTIFF
        let output_file = self
            .output_path
            .join(file_name.unwrap_or("lidar_cdsm.tif".to_string()));
        let output_path = self.to_tif(&rasters, &output_file, write_out_file)?;

        Ok(output_path)
    }

    /// Export loaded LiDAR points (ROI of the current BBOX) to a LAS file.
    /// Call set_bbox() first to load points. Use .las for uncompressed or .laz for compressed output.
    #[cfg(feature = "las")]
    pub fn save_las(&self, path: &Path) -> Result<PathBuf> {
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded. Call set_bbox() first.")?;
        if points.is_empty() {
            anyhow::bail!("No LiDAR points to export. Call set_bbox() first.");
        }

        let (min_x, min_y, min_z, _max_x, _max_y, _max_z) = points.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, min_y, min_z, max_x, max_y, max_z), p| {
                (
                    min_x.min(p.x),
                    min_y.min(p.y),
                    min_z.min(p.z),
                    max_x.max(p.x),
                    max_y.max(p.y),
                    max_z.max(p.z),
                )
            },
        );

        let mut builder = las::Builder::from((1, 4));
        // Format 0: no GPS time, no RGB — matches our Point (classification + return_number only).
        builder.point_format = las::point::Format::new(0).context("Invalid point format")?;
        builder.transforms = las::Vector {
            x: las::Transform {
                scale: 0.01,
                offset: min_x,
            },
            y: las::Transform {
                scale: 0.01,
                offset: min_y,
            },
            z: las::Transform {
                scale: 0.01,
                offset: min_z,
            },
        };
        let header = builder
            .into_header()
            .context("Failed to build LAS header")?;

        let out_path: PathBuf = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.output_path.join(path)
        };

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create output directory for LAS")?;
        }

        let mut writer =
            las::Writer::from_path(&out_path, header).context("Failed to create LAS writer")?;

        for p in points {
            let classification = las::point::Classification::new(p.classification)
                .unwrap_or(las::point::Classification::Unclassified);
            let las_point = las::Point {
                x: p.x,
                y: p.y,
                z: p.z,
                classification,
                return_number: 1,
                number_of_returns: 1,
                ..Default::default()
            };
            writer.write_point(las_point).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to write LAS point (check coordinates/format): {}",
                    e
                )
            })?;
        }

        writer.close().context("Failed to close LAS writer")?;
        Ok(out_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn test_spatial_grid_index() {
        // Create test points
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

        // Build index with 10m cell size
        let index = SpatialGridIndex::build_from_points(&points, 10.0);

        // Query for points in [0, 0] to [12, 12]
        let candidates = index.query_bbox(0.0, 0.0, 12.0, 12.0);

        // Should return indices for first 3 points
        assert!(candidates.contains(&0));
        assert!(candidates.contains(&1));
        assert!(candidates.contains(&2));

        // Point at (100, 100) should not be in results
        assert!(!candidates.contains(&4));
    }

    #[test]
    fn test_quadtree_spatial_index() {
        // Create test points in a grid pattern
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

        // Build quadtree
        let quadtree = QuadtreeSpatialIndex::build(&points);

        // Query small bbox
        let candidates = quadtree.query_bbox(45.0, 45.0, 55.0, 55.0);

        // Should have ~4 points (those at 50,50 area)
        assert!(!candidates.is_empty());
        assert!(candidates.len() < 100); // Much less than total

        // Verify all candidates are actually in or near bbox
        for &idx in &candidates {
            let p = &points[idx];
            // Points should be near the query bbox (within one cell)
            assert!(p.x >= 40.0 && p.x <= 60.0);
            assert!(p.y >= 40.0 && p.y <= 60.0);
        }
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
        // Small dataset - should use linear scan
        let points: Vec<LidarPoint> = (0..100)
            .map(|i| LidarPoint {
                x: i as f64,
                y: i as f64,
                z: i as f64,
                classification: 2,
            })
            .collect();

        let filtered = Lidar::filter_points_with_spatial_index(&points, 25.0, 25.0, 75.0, 75.0);

        // Should have points from 25 to 75 inclusive
        assert_eq!(filtered.len(), 51);
        assert!(filtered.iter().all(|p| p.x >= 25.0 && p.x <= 75.0));
    }

    #[test]
    fn test_filter_points_with_spatial_index_large() {
        // Large dataset - should use spatial index
        let points: Vec<LidarPoint> = (0..50_000)
            .map(|i| LidarPoint {
                x: (i % 1000) as f64,
                y: (i / 1000) as f64 * 10.0,
                z: i as f64 * 0.1,
                classification: 2,
            })
            .collect();

        let filtered = Lidar::filter_points_with_spatial_index(&points, 100.0, 100.0, 200.0, 200.0);

        // All filtered points should be in bbox
        assert!(filtered
            .iter()
            .all(|p| p.x >= 100.0 && p.x <= 200.0 && p.y >= 100.0 && p.y <= 200.0));
    }

    #[test]
    fn test_octree_node_quadrant() {
        let node = OctreeNode::new_leaf((0.0, 0.0, 0.0, 100.0, 100.0, 100.0), 0);

        // SW quadrant
        assert_eq!(node.quadrant_for_point(25.0, 25.0), 0);
        // SE quadrant
        assert_eq!(node.quadrant_for_point(75.0, 25.0), 1);
        // NW quadrant
        assert_eq!(node.quadrant_for_point(25.0, 75.0), 2);
        // NE quadrant
        assert_eq!(node.quadrant_for_point(75.0, 75.0), 3);
    }
}
