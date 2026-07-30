use anyhow::{Context, Result};
use laz::laszip::LazVlr;
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
    cell_size: f64,
    origin_x: f64,
    origin_y: f64,
    cells: HashMap<GridCellKey, Vec<usize>>,
    point_count: usize,
}

impl SpatialGridIndex {
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

    #[inline]
    fn cell_key(&self, x: f64, y: f64) -> GridCellKey {
        GridCellKey {
            col: ((x - self.origin_x) / self.cell_size).floor() as i64,
            row: ((y - self.origin_y) / self.cell_size).floor() as i64,
        }
    }

    fn build_from_points(points: &[LidarPoint], cell_size: f64) -> Self {
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
        for (i, p) in points.iter().enumerate() {
            let key = index.cell_key(p.x, p.y);
            index.cells.entry(key).or_insert_with(Vec::new).push(i);
        }
        index.point_count = points.len();
        index
    }

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

#[derive(Debug)]
struct OctreeNode {
    bounds: (f64, f64, f64, f64, f64, f64),
    points: Vec<usize>,
    children: Option<Box<[Option<OctreeNode>; 4]>>,
    depth: u8,
}

impl OctreeNode {
    const MAX_POINTS_PER_NODE: usize = 1000;
    const MAX_DEPTH: u8 = 12;

    fn new_leaf(bounds: (f64, f64, f64, f64, f64, f64), depth: u8) -> Self {
        OctreeNode {
            bounds,
            points: Vec::new(),
            children: None,
            depth,
        }
    }

    #[inline]
    fn contains_xy(&self, x: f64, y: f64) -> bool {
        x >= self.bounds.0 && x <= self.bounds.3 && y >= self.bounds.1 && y <= self.bounds.4
    }

    #[inline]
    fn intersects_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        !(self.bounds.3 < min_x
            || self.bounds.0 > max_x
            || self.bounds.4 < min_y
            || self.bounds.1 > max_y)
    }

    fn quadrant_for_point(&self, x: f64, y: f64) -> usize {
        let mid_x = (self.bounds.0 + self.bounds.3) / 2.0;
        let mid_y = (self.bounds.1 + self.bounds.4) / 2.0;
        match (x >= mid_x, y >= mid_y) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

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
            ),
            1 => (
                mid_x,
                self.bounds.1,
                self.bounds.2,
                self.bounds.3,
                mid_y,
                self.bounds.5,
            ),
            2 => (
                self.bounds.0,
                mid_y,
                self.bounds.2,
                mid_x,
                self.bounds.4,
                self.bounds.5,
            ),
            3 => (
                mid_x,
                mid_y,
                self.bounds.2,
                self.bounds.3,
                self.bounds.4,
                self.bounds.5,
            ),
            _ => unreachable!(),
        }
    }

    fn insert(&mut self, point_idx: usize, points: &[LidarPoint]) {
        let point = &points[point_idx];
        if !self.contains_xy(point.x, point.y) {
            return;
        }
        if self.children.is_some() {
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
        self.points.push(point_idx);
        if self.points.len() > Self::MAX_POINTS_PER_NODE && self.depth < Self::MAX_DEPTH {
            self.split(points);
        }
    }

    fn split(&mut self, points: &[LidarPoint]) {
        self.children = Some(Box::new([None, None, None, None]));
        let old_points = std::mem::take(&mut self.points);
        for point_idx in old_points {
            self.insert(point_idx, points);
        }
    }

    fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, result: &mut Vec<usize>) {
        if !self.intersects_bbox(min_x, min_y, max_x, max_y) {
            return;
        }
        result.extend(&self.points);
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                child.query_bbox(min_x, min_y, max_x, max_y, result);
            }
        }
    }

    fn count_points(&self) -> usize {
        let mut count = self.points.len();
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                count += child.count_points();
            }
        }
        count
    }

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

#[derive(Debug)]
pub struct QuadtreeSpatialIndex {
    root: OctreeNode,
}

impl QuadtreeSpatialIndex {
    pub(crate) fn build(points: &[LidarPoint]) -> Self {
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
        let padding = 0.001;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;
        let mut root = OctreeNode::new_leaf((min_x, min_y, min_z, max_x, max_y, max_z), 0);
        for i in 0..points.len() {
            root.insert(i, points);
        }
        QuadtreeSpatialIndex { root }
    }

    pub fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let mut result = Vec::new();
        self.root
            .query_bbox(min_x, min_y, max_x, max_y, &mut result);
        result
    }

    pub fn stats(&self) -> String {
        format!(
            "Quadtree: {} nodes, {} points indexed",
            self.root.count_nodes(),
            self.root.count_points()
        )
    }
}

// ============================================================================
// COPC NATIVE READER
//
// Implémentation directe de la spec COPC 1.0 (https://copc.io/),
// inspirée de copc-rs 0.5.0 (pka/copc-rs) mais sans aucune dépendance externe.
//
// Problème résolu : las::CopcEntryReader échoue sur 99.9% des entries des
// fichiers IGN LiDAR HD car il réinitialise le décompresseur par entry, ce qui
// est incompatible avec LAS 1.4 Format 6+ (LAZ chunked sans chunk table par entry).
//
// Solution (identique à copc-rs/decompressor.rs) :
//   Utiliser laz::record::LayeredPointRecordDecompressor SANS lire de chunk table.
//   Chaque entry COPC est un chunk LAZ autonome adressé par son offset absolu.
//   On crée un décompresseur frais par chunk, on pointe sur chunk_buf, on lit
//   entry.point_count points — sans jamais toucher à un chunk table global.
//
// Structure du fichier COPC :
//   [LAS 1.4 header       : 375 bytes @ offset 0]
//   [COPC Info VLR header : 54 bytes  @ offset 375]   user_id="copc" record_id=1
//   [COPC Info VLR data   : 160 bytes @ offset 429]
//   [LAZ VLR              : quelque part après 375]    user_id="laszip encoded" record_id=22204
//   [Point data (LAZ)     : @ header.offset_to_point_data]
//   [COPC Hierarchy EVLR  : @ header.evlr_offset]      entries de 32 bytes chacune
// ============================================================================

/// LAS 1.4 public header (375 bytes).
#[derive(Debug, Clone)]
struct Las14Header {
    /// Format de point (bits 7-6 masqués = flag LAZ)
    point_format: u8,
    /// Taille d'un enregistrement de point en bytes
    point_record_len: u16,
    /// Offset vers les données de points (bytes 96-99, u32 LE)
    offset_to_point_data: u64,
    /// Nombre de VLRs (bytes 100-103)
    vlr_count: u32,
    /// Échelles XYZ (bytes 131/139/147, f64 LE)
    scale: [f64; 3],
    /// Offsets de coordonnées XYZ (bytes 155/163/171, f64 LE)
    coord_offset: [f64; 3],
    /// Offset vers le premier EVLR (bytes 235-242, u64 LE)
    evlr_offset: u64,
    /// Nombre de points 64-bit LAS 1.4 (bytes 247-254, u64 LE)
    point_count: u64,
}

/// COPC Info VLR — 160 bytes de données (offset fichier 429).
#[derive(Debug, Clone)]
struct CopcInfo {
    center_x: f64,
    center_y: f64,
    center_z: f64,
    halfsize: f64,
    #[allow(dead_code)]
    spacing: f64,
    /// Offset absolu dans le fichier vers la page racine de hiérarchie
    root_hier_offset: u64,
    /// Taille en bytes de la page racine
    root_hier_size: u64,
}

/// Une entrée dans la hiérarchie COPC (32 bytes).
#[derive(Debug, Clone)]
struct CopcEntry {
    /// Niveau de l'octree (0 = racine)
    level: i32,
    /// Coordonnées de voxel X
    vx: i32,
    /// Coordonnées de voxel Y
    vy: i32,
    #[allow(dead_code)]
    vz: i32,
    /// Offset absolu dans le fichier vers le chunk LAZ compressé
    offset: u64,
    /// Taille compressée en bytes ; si -1 = page enfant de hiérarchie
    byte_size: i32,
    /// Nombre de points ; -1 = page enfant ; 0 = nœud vide
    point_count: i32,
}

// --- helpers binaires little-endian ---
#[inline]
fn ru16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
#[inline]
fn ru32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
#[inline]
fn ru64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
#[inline]
fn ri32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
#[inline]
fn rf64(b: &[u8], o: usize) -> f64 {
    f64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Parse le LAS 1.4 public header depuis un buffer ≥ 375 bytes.
fn parse_las14_header(buf: &[u8]) -> Result<Las14Header> {
    if buf.len() < 375 {
        anyhow::bail!("Buffer trop court pour LAS 1.4 header: {} bytes", buf.len());
    }
    if &buf[0..4] != b"LASF" {
        anyhow::bail!("Signature LASF manquante");
    }
    if buf[25] < 4 {
        anyhow::bail!("COPC requiert LAS 1.4, trouvé 1.{}", buf[25]);
    }
    Ok(Las14Header {
        point_format: buf[104] & 0x3F,
        point_record_len: ru16(buf, 105),
        offset_to_point_data: ru32(buf, 96) as u64,
        vlr_count: ru32(buf, 100),
        scale: [rf64(buf, 131), rf64(buf, 139), rf64(buf, 147)],
        coord_offset: [rf64(buf, 155), rf64(buf, 163), rf64(buf, 171)],
        evlr_offset: ru64(buf, 235),
        point_count: ru64(buf, 247),
    })
}

/// Parse le COPC Info VLR depuis 160 bytes (data seule, sans les 54 bytes de VLR header).
fn parse_copc_info(buf: &[u8]) -> Result<CopcInfo> {
    if buf.len() < 160 {
        anyhow::bail!("COPC Info VLR trop court: {} bytes", buf.len());
    }
    Ok(CopcInfo {
        center_x: rf64(buf, 0),
        center_y: rf64(buf, 8),
        center_z: rf64(buf, 16),
        halfsize: rf64(buf, 24),
        spacing: rf64(buf, 32),
        root_hier_offset: ru64(buf, 40),
        root_hier_size: ru64(buf, 48),
    })
}

/// Parse une page de hiérarchie COPC (entrées de 32 bytes chacune).
fn parse_hierarchy_page(buf: &[u8]) -> Vec<CopcEntry> {
    let n = buf.len() / 32;
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        let b = i * 32;
        entries.push(CopcEntry {
            level: ri32(buf, b),
            vx: ri32(buf, b + 4),
            vy: ri32(buf, b + 8),
            vz: ri32(buf, b + 12),
            offset: ru64(buf, b + 16),
            byte_size: ri32(buf, b + 24),
            point_count: ri32(buf, b + 28),
        });
    }
    entries
}

/// Lit toutes les entrées de hiérarchie COPC en suivant les pages enfants.
/// Les entrées avec point_count == -1 sont des pages enfants à lire récursivement.
fn read_all_hierarchy_entries(file_buf: &[u8], info: &CopcInfo) -> Result<Vec<CopcEntry>> {
    let mut all_entries = Vec::new();
    let mut pages_to_read: Vec<(u64, u64)> = vec![(info.root_hier_offset, info.root_hier_size)];

    while let Some((page_offset, page_size)) = pages_to_read.pop() {
        let start = page_offset as usize;
        let end = start + page_size as usize;
        if end > file_buf.len() {
            eprintln!(
                "  ⚠️  Page hiérarchie COPC hors limites: offset={} size={} file_len={}",
                page_offset,
                page_size,
                file_buf.len()
            );
            continue;
        }
        for entry in parse_hierarchy_page(&file_buf[start..end]) {
            if entry.point_count == -1 {
                // Page enfant — lire récursivement
                pages_to_read.push((entry.offset, entry.byte_size as u64));
            } else {
                all_entries.push(entry);
            }
        }
    }
    Ok(all_entries)
}

/// Calcule le bounding box XY d'un nœud COPC.
/// Formule COPC 1.0 / laspy : le cube racine couvre [center ± halfsize],
/// subdivisé en 2^level cellules de côté (2 * halfsize) / 2^level.
fn copc_entry_bounds_xy(entry: &CopcEntry, info: &CopcInfo) -> (f64, f64, f64, f64) {
    let side_size = (2.0 * info.halfsize) / (1u64 << entry.level.max(0)) as f64;
    let root_min_x = info.center_x - info.halfsize;
    let root_min_y = info.center_y - info.halfsize;
    let min_x = root_min_x + entry.vx as f64 * side_size;
    let min_y = root_min_y + entry.vy as f64 * side_size;
    (min_x, min_y, min_x + side_size, min_y + side_size)
}

/// Teste si un nœud COPC intersecte le bbox de requête (XY uniquement).
fn copc_entry_intersects(
    entry: &CopcEntry,
    info: &CopcInfo,
    qx_min: f64,
    qy_min: f64,
    qx_max: f64,
    qy_max: f64,
) -> bool {
    let (nx_min, ny_min, nx_max, ny_max) = copc_entry_bounds_xy(entry, info);
    !(nx_max < qx_min || nx_min > qx_max || ny_max < qy_min || ny_min > qy_max)
}

/// Cherche et parse le LazVlr dans le buffer du fichier.
/// user_id = "laszip encoded" (16 bytes null-padded), record_id = 22204.
/// Les VLRs commencent à l'offset 375 (taille du LAS 1.4 header).
fn find_laz_vlr(file_buf: &[u8], vlr_count: u32) -> Result<LazVlr> {
    const LAS14_HEADER_SIZE: usize = 375;
    const VLR_HEADER_SIZE: usize = 54;

    let mut pos = LAS14_HEADER_SIZE;

    for _ in 0..vlr_count {
        if pos + VLR_HEADER_SIZE > file_buf.len() {
            break;
        }
        // VLR header layout:
        //   reserved (2) | user_id (16) | record_id (2) | record_len (2) | description (32)
        let user_id = std::str::from_utf8(&file_buf[pos + 2..pos + 18])
            .unwrap_or("")
            .trim_end_matches('\0');
        let record_id = ru16(file_buf, pos + 18);
        let record_len = ru16(file_buf, pos + 20) as usize;
        let data_start = pos + VLR_HEADER_SIZE;
        let data_end = data_start + record_len;

        if user_id == "laszip encoded" && record_id == 22204 {
            if data_end > file_buf.len() {
                anyhow::bail!(
                    "LAZ VLR data tronquée (data_end={} > file_len={})",
                    data_end,
                    file_buf.len()
                );
            }
            let vlr = LazVlr::from_buffer(&file_buf[data_start..data_end])
                .map_err(|e| anyhow::anyhow!("Échec parse LazVlr: {}", e))?;
            return Ok(vlr);
        }
        pos = data_end;
    }
    anyhow::bail!(
        "LazVlr introuvable parmi {} VLRs (user_id='laszip encoded', record_id=22204)",
        vlr_count
    )
}

/// Décompresse un chunk COPC en bytes bruts.
///
/// Implémentation clé tirée de copc-rs/decompressor.rs :
///   - On crée un LayeredPointRecordDecompressor sur chunk_buf (Cursor)
///   - On appelle set_fields_from(vlr.items()) pour configurer les canaux
///   - On lit point_count fois decompress_next() — SANS lire de chunk table
///
/// C'est fondamentalement différent de LasZipDecompressor qui attend un
/// chunk table en début de données. Les chunks COPC n'ont pas de chunk table
/// individuel : ils font partie d'un pool géré par le fichier global.
fn decompress_copc_chunk(
    chunk_buf: &[u8],
    vlr: &LazVlr,
    point_record_len: u16,
    point_count: i32,
) -> Result<Vec<u8>> {
    use laz::record::{LayeredPointRecordDecompressor, RecordDecompressor};
    use std::io::Cursor;

    let mut cursor = Cursor::new(chunk_buf);
    let mut decompressor = LayeredPointRecordDecompressor::new(&mut cursor);
    decompressor
        .set_fields_from(vlr.items())
        .map_err(|e| anyhow::anyhow!("set_fields_from: {}", e))?;

    let point_size = point_record_len as usize;
    let n = point_count as usize;
    let mut out = vec![0u8; n * point_size];

    for i in 0..n {
        let slice = &mut out[i * point_size..(i + 1) * point_size];
        decompressor
            .decompress_next(slice)
            .map_err(|e| anyhow::anyhow!("Décompression point {}/{}: {}", i + 1, n, e))?;
    }
    Ok(out)
}

/// Décode un point LAS 1.4 Format 6/7/8 depuis des bytes bruts.
///
/// Layout Format 6 (20 bytes minimum) :
///   bytes  0- 3 : X (i32 LE)
///   bytes  4- 7 : Y (i32 LE)
///   bytes  8-11 : Z (i32 LE)
///   bytes 12-13 : intensity (u16) — non utilisé
///   byte  14    : return number bits — non utilisé
///   byte  15    : flags — non utilisé
///   byte  16    : classification (u8)
///   ...          (scanner channel, scan angle, user data, point source, gps time, etc.)
#[inline]
fn decode_las14_point(raw: &[u8], scale: &[f64; 3], coord_offset: &[f64; 3]) -> LidarPoint {
    let xi = i32::from_le_bytes(raw[0..4].try_into().unwrap());
    let yi = i32::from_le_bytes(raw[4..8].try_into().unwrap());
    let zi = i32::from_le_bytes(raw[8..12].try_into().unwrap());
    LidarPoint {
        x: xi as f64 * scale[0] + coord_offset[0],
        y: yi as f64 * scale[1] + coord_offset[1],
        z: zi as f64 * scale[2] + coord_offset[2],
        classification: raw[16],
    }
}

/// Statistiques de lecture COPC
struct CopcReadStats {
    points: Vec<LidarPoint>,
    entries_processed: usize,
    entries_success: usize,
    entries_failed: usize,
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

pub struct Lidar {
    pub geo_core: GeoCore,
    output_path: PathBuf,
    classification: Option<u8>,
    list_path_laz: Option<Vec<String>>,
    loaded_points: Option<Vec<LidarPoint>>,
}

#[derive(Debug, Clone)]
pub(crate) struct LidarPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    pub(crate) classification: u8,
}

const LAS_HEADER_MIN_BYTES: usize = 111;
const LAS_OFFSET_TO_POINT_DATA: usize = 94;
const LAS_NUMBER_OF_POINT_RECORDS: usize = 107;

#[derive(Debug)]
struct LasHeaderParsed {
    offset_to_point_data: u32,
    #[allow(dead_code)]
    number_of_points: u64,
}

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

#[cfg(feature = "laz-memmap")]
const LAZ_MMAP_THRESHOLD_BYTES: u64 = 50 * 1024 * 1024;

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
fn head_content_length(client: &reqwest::blocking::Client, url: &str) -> Option<u64> {
    let response = client.head(url).send().ok()?;
    // Some CDNs (e.g. data.geopf.fr) answer HEAD with Content-Length: 0.
    // Treat 0 / missing as unknown so callers don't divide by zero or retry forever.
    response.content_length().filter(|&n| n > 0)
}

/// Preserve ASPRS + IGN LiDAR HD extended codes (64, 66, 67, …).
#[inline]
fn classification_to_u8(c: &las::point::Classification) -> u8 {
    u8::from(*c)
}

struct ProcessedRasters {
    dsm: Vec<Vec<f64>>,
    dtm: Vec<Vec<f64>>,
    chm: Vec<Vec<f64>>,
    width: usize,
    height: usize,
    transform: [f64; 6],
}

impl Lidar {
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
            geo_core: GeoCore::default(),
            output_path: output_path_buf,
            classification,
            list_path_laz: None,
            loaded_points: None,
        };
        if let Some((min_x, min_y, max_x, max_y)) = bbox {
            lidar.set_bbox(min_x, min_y, max_x, max_y)?;
        }
        Ok(lidar)
    }

    /// Set bbox (WGS84) and resolve COPC/LAZ URLs via IGN WFS.
    /// Does **not** download point data — that happens on first `run()` / `save_las()`.
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<()> {
        self.geo_core
            .set_bbox(Some(BoundingBox::new(min_x, min_y, max_x, max_y)));
        self.loaded_points = None;
        let _ = self.get_lidar_points()?;
        Ok(())
    }

    /// Download and cache points for the current bbox if not already loaded.
    fn ensure_points_loaded(&mut self) -> Result<()> {
        if self.loaded_points.is_some() {
            return Ok(());
        }
        let laz_urls = self
            .list_path_laz
            .as_ref()
            .context("No LAZ URLs available. Call set_bbox() first.")?;
        if laz_urls.is_empty() {
            anyhow::bail!("No LAZ files found for the specified bounding box");
        }
        let laz_urls = laz_urls.clone();
        let (min_x, min_y, max_x, max_y) = self.bbox_lambert93()?;
        let points = self.load_lidar_points_internal(&laz_urls, Some((min_x, min_y, max_x, max_y)))?;
        self.loaded_points = Some(points);
        Ok(())
    }

    /// Current bbox transformed to EPSG:2154 (Lambert-93).
    fn bbox_lambert93(&self) -> Result<(f64, f64, f64, f64)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set")?;
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;
        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;
        Ok((min_x, min_y, max_x, max_y))
    }

    pub fn set_classification(&mut self, classification: Option<u8>) {
        self.classification = classification;
    }

    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }

    /// COPC tile URLs for the current bbox (populated by `set_bbox` / ctor with bbox).
    ///
    /// Example: `https://data.geopf.fr/.../LHD_FXX_0399_6580_PTS_LAMB93_IGN69.copc.laz`
    pub fn list_copc_urls(&self) -> Result<Vec<String>> {
        let urls = self.list_path_laz.as_ref().context(
            "No COPC/LAZ URLs loaded. Call set_bbox() first.",
        )?;
        Ok(urls
            .iter()
            .filter(|u| Self::is_copc_url(u))
            .cloned()
            .collect())
    }

    /// All LAZ tile URLs (COPC and non-COPC) for the current bbox.
    pub fn list_laz_urls(&self) -> Result<Vec<String>> {
        self.list_path_laz
            .clone()
            .context("No LAZ URLs loaded. Call set_bbox() first.")
    }

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

    fn is_copc_url(url: &str) -> bool {
        url.ends_with(".copc.laz") || url.contains(".copc.")
    }

    fn verify_cached_file(cache_path: &Path) -> Result<bool> {
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
    fn download_with_verification(
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

    fn load_single_point_file(
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
    fn load_single_copc_file(
        &self,
        url: &str,
        cache_dir: &Path,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<Vec<LidarPoint>> {
        let cache_path = Self::cache_path_for_url(cache_dir, url);

        // --- 1. Obtenir les bytes (cache ou téléchargement) ---
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
        let stats =
            Self::decompress_copc_entries(&bytes, &entries_to_process, &laz_vlr, &hdr, filter_bbox);

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

    /// Décompresse les entries COPC sélectionnées et retourne les points avec statistiques.
    fn decompress_copc_entries(
        file_buf: &[u8],
        entries: &[&CopcEntry],
        laz_vlr: &LazVlr,
        hdr: &Las14Header,
        filter_bbox: Option<(f64, f64, f64, f64)>,
    ) -> CopcReadStats {
        let mut result_points: Vec<LidarPoint> = Vec::new();
        let mut entries_processed = 0usize;
        let mut entries_success = 0usize;
        let mut entries_failed = 0usize;

        for (idx, entry) in entries.iter().enumerate() {
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
                entries_processed += 1;
                entries_failed += 1;
                continue;
            }

            entries_processed += 1;
            let chunk_buf = &file_buf[chunk_start..chunk_end];

            match decompress_copc_chunk(chunk_buf, laz_vlr, hdr.point_record_len, entry.point_count)
            {
                Ok(raw_points) => {
                    entries_success += 1;
                    let point_size = hdr.point_record_len as usize;
                    let n = entry.point_count as usize;

                    for i in 0..n {
                        let raw = &raw_points[i * point_size..(i + 1) * point_size];
                        let pt = decode_las14_point(raw, &hdr.scale, &hdr.coord_offset);

                        // Filtre spatial précis post-décompression
                        if let Some((qx_min, qy_min, qx_max, qy_max)) = filter_bbox {
                            if pt.x < qx_min || pt.x > qx_max || pt.y < qy_min || pt.y > qy_max {
                                continue;
                            }
                        }
                        result_points.push(pt);
                    }
                }
                Err(e) => {
                    entries_failed += 1;
                    // Log seulement les 10 premières erreurs pour ne pas noyer les logs
                    if entries_failed <= 10 {
                        eprintln!(
                            "  ⚠️  Chunk {} (level={} vx={} vy={}): {}",
                            idx, entry.level, entry.vx, entry.vy, e
                        );
                    }
                }
            }

            // Progress tous les 500 chunks
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

        let file_points = if let Some((x_min, y_min, x_max, y_max)) = filter_bbox {
            Self::filter_points_with_spatial_index(&all_points, x_min, y_min, x_max, y_max)
        } else {
            all_points
        };
        println!("  Loaded {} points after spatial filter", file_points.len());
        Ok(file_points)
    }

    fn filter_points_with_spatial_index(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();
        if point_count < 10_000 {
            return points
                .iter()
                .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
                .cloned()
                .collect();
        }
        println!("  Building spatial index for {} points...", point_count);
        let start = std::time::Instant::now();
        let query_area = (x_max - x_min) * (y_max - y_min);
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
        let result = if selectivity > 0.5 || point_count < 100_000 {
            let cell_size = ((data_max_x - data_min_x) / 100.0)
                .max((data_max_y - data_min_y) / 100.0)
                .max(10.0);
            let grid_index = SpatialGridIndex::build_from_points(points, cell_size);
            println!("  {}", grid_index.stats());
            let candidate_indices = grid_index.query_bbox(x_min, y_min, x_max, y_max);
            println!(
                "  🔍 Grid query returned {} candidates",
                candidate_indices.len()
            );
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
            let quadtree = QuadtreeSpatialIndex::build(points);
            println!("  {}", quadtree.stats());
            let candidate_indices = quadtree.query_bbox(x_min, y_min, x_max, y_max);
            println!(
                "  🔍 Quadtree query returned {} candidates",
                candidate_indices.len()
            );
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
        println!(
            "  Spatial indexing and query took {:.2}s",
            start.elapsed().as_secs_f64()
        );
        result
    }

    #[cfg(feature = "rayon")]
    fn filter_points_with_spatial_index_parallel(
        points: &[LidarPoint],
        x_min: f64,
        y_min: f64,
        x_max: f64,
        y_max: f64,
    ) -> Vec<LidarPoint> {
        let point_count = points.len();
        if point_count < 1_000_000 {
            return Self::filter_points_with_spatial_index(points, x_min, y_min, x_max, y_max);
        }
        println!(
            "  🚀 Using parallel spatial indexing for {} points...",
            point_count
        );
        let start = std::time::Instant::now();
        let chunk_size = 500_000;
        let chunks: Vec<_> = points.chunks(chunk_size).collect();
        let results: Vec<Vec<LidarPoint>> = chunks
            .par_iter()
            .map(|chunk| Self::filter_points_with_spatial_index(chunk, x_min, y_min, x_max, y_max))
            .collect();
        let result: Vec<LidarPoint> = results.into_iter().flatten().collect();
        println!(
            "  Parallel spatial filtering took {:.2}s",
            start.elapsed().as_secs_f64()
        );
        result
    }

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

    #[cfg(feature = "reqwest")]
    fn download_laz_via_range(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
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
            match Self::verify_cached_file(&cache_path) {
                Ok(true) => {
                    println!("Reading LAZ from cache: {:?}", cache_path);
                }
                Ok(false) | Err(_) => {
                    eprintln!("  Cached file appears corrupted, removing...");
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
                    if data.len() < 4 || &data[0..4] != b"LASF" {
                        return Err(anyhow::anyhow!("Downloaded file is not a valid LAS/LAZ"));
                    }
                    std::fs::write(&cache_path, &data).context("Failed to write LAZ cache")?;
                    println!("  Cached to: {:?}", cache_path);
                    data
                }
                Err(_) => Self::download_with_verification(&client, url, &cache_path)?,
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

    fn get_lidar_points(&mut self) -> Result<(f64, f64, f64, f64, Vec<String>)> {
        let bbox = self
            .geo_core
            .get_bbox()
            .context("Bounding box must be set before getting LiDAR points")?;
        println!("Bounding box set");
        let transformer = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create coordinate transformer")?;
        let (min_x, min_y) = transformer
            .convert((bbox.min_x, bbox.min_y))
            .context("Failed to transform min coordinates")?;
        let (max_x, max_y) = transformer
            .convert((bbox.max_x, bbox.max_y))
            .context("Failed to transform max coordinates")?;
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
        self.list_path_laz = Some(list_path_laz.clone());
        Ok((min_x, min_y, max_x, max_y, list_path_laz))
    }

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

            // COPC : séquentiel pour éviter le rate-limiting IGN (HTTP 429)
            // LAZ standard : parallèle comme avant
            let (copc_urls, laz_only_urls): (Vec<_>, Vec<_>) =
                laz_urls.iter().partition(|u| Self::is_copc_url(u));

            let mut all_points: Vec<LidarPoint> = Vec::new();
            let mut tile_point_counts: Vec<(String, usize)> = Vec::new();

            // --- COPC séquentiel avec retry/backoff ---
            for (idx, url) in copc_urls.iter().enumerate() {
                println!("\n📦 COPC {}/{}: {}", idx + 1, copc_urls.len(), url);
                let mut attempt = 0u32;
                let mut tile_points = 0usize;
                loop {
                    attempt += 1;
                    match self.load_single_copc_file(url, &cache_dir, filter_bbox) {
                        Ok(pts) => {
                            tile_points = pts.len();
                            println!("  ✅ {} points chargés", tile_points);
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
                                eprintln!(
                                    "  ⚠️  Erreur (tentative {}/5): {}. Retry dans {}s...",
                                    attempt, msg, wait
                                );
                                std::thread::sleep(std::time::Duration::from_secs(wait));
                            } else {
                                eprintln!("  ❌ Échec COPC après {} tentative(s): {}", attempt, e);
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
                #[cfg(feature = "indicatif")]
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
                        #[cfg(feature = "indicatif")]
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

            #[cfg(feature = "indicatif")]
            if let Some(ref pb) = overall_pb {
                pb.lock()
                    .unwrap()
                    .finish_with_message("All files processed");
            }

            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }
            println!("Total points loaded: {}", all_points.len());
            return Ok(all_points);
        }

        #[cfg(not(feature = "rayon"))]
        {
            let mut all_points = Vec::new();
            let mut tile_point_counts: Vec<(String, usize)> = Vec::new();

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
                let mut attempt = 0u32;
                let mut tile_points = 0usize;
                loop {
                    attempt += 1;
                    match self.load_single_point_file(url, &cache_dir, filter_bbox) {
                        Ok(points) => {
                            tile_points = points.len();
                            println!("  Loaded {} points", tile_points);
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
                                eprintln!("  ⚠️  Retry {}/5 dans {}s: {}", attempt, wait, msg);
                                std::thread::sleep(std::time::Duration::from_secs(wait));
                            } else {
                                eprintln!("  ❌ Failed to load: {}", e);
                                break;
                            }
                        }
                    }
                }
                tile_point_counts.push(((*url).clone(), tile_points));
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

            Self::validate_tile_coverage(laz_urls, &tile_point_counts)?;

            if all_points.is_empty() {
                anyhow::bail!("No LiDAR points were loaded from any file");
            }
            println!("\nTotal points loaded: {}", all_points.len());
            Ok(all_points)
        }
    }

    /// Warn on empty tiles; fail when fewer than half of WFS tiles contributed points.
    fn validate_tile_coverage(
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
            eprintln!("  ⚠️  Tile returned 0 points: {}", url);
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

    fn process_lidar_points(
        &self,
        points: Vec<LidarPoint>,
        bbox: (f64, f64, f64, f64),
        classification_list: Option<Vec<u8>>,
        resolution: f64,
    ) -> Result<ProcessedRasters> {
        let (x_min, y_min, x_max, y_max) = bbox;
        let filtered_points: Vec<LidarPoint> = points
            .into_iter()
            .filter(|p| p.x >= x_min && p.x <= x_max && p.y >= y_min && p.y <= y_max)
            .collect();
        println!("Filtered {} points within bbox", filtered_points.len());
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
        let width = ((x_max - x_min) / resolution).ceil() as usize;
        let height = ((y_max - y_min) / resolution).ceil() as usize;
        println!(
            "Grid dimensions: {}x{} (resolution: {}m)",
            width, height, resolution
        );
        let mut dsm = vec![vec![f64::NEG_INFINITY; width]; height];
        let mut dtm = vec![vec![f64::NEG_INFINITY; width]; height];
        for point in &filtered_points {
            let col = ((point.x - x_min) / resolution).floor() as usize;
            let row = ((y_max - point.y) / resolution).floor() as usize;
            if col < width && row < height {
                if point.z > dsm[row][col] || dsm[row][col] == f64::NEG_INFINITY {
                    dsm[row][col] = point.z;
                }
                if point.classification == 2 {
                    if point.z > dtm[row][col] || dtm[row][col] == f64::NEG_INFINITY {
                        dtm[row][col] = point.z;
                    }
                }
            }
        }
        let mut dtm_filled = dtm.clone();
        for row in 0..height {
            for col in 0..width {
                if dtm_filled[row][col] == f64::NEG_INFINITY {
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
                    dtm_filled[row][col] = if min_neighbor != f64::INFINITY {
                        min_neighbor
                    } else {
                        0.0
                    };
                }
            }
        }
        let mut chm = vec![vec![0.0; width]; height];
        for row in 0..height {
            for col in 0..width {
                if dsm[row][col] != f64::NEG_INFINITY && dtm_filled[row][col] != f64::NEG_INFINITY {
                    chm[row][col] = (dsm[row][col] - dtm_filled[row][col]).max(0.0);
                }
            }
        }
        Ok(ProcessedRasters {
            dsm,
            dtm: dtm_filled,
            chm,
            width,
            height,
            transform: [x_min, resolution, 0.0, y_max, 0.0, -resolution],
        })
    }

    fn to_tif(
        &self,
        rasters: &ProcessedRasters,
        output_path: &Path,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        use gdal::raster::Buffer;
        use gdal::spatial_ref::SpatialRef;
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }
        if write_out_file {
            let driver = gdal::DriverManager::get_driver_by_name("GTiff")
                .context("Failed to get GTiff driver")?;
            let mut dataset = driver
                .create_with_band_type::<f64, _>(output_path, rasters.width, rasters.height, 3)
                .context("Failed to create GeoTIFF dataset")?;
            dataset
                .set_geo_transform(&rasters.transform)
                .context("Failed to set geotransform")?;
            let srs = SpatialRef::from_epsg(self.geo_core.get_epsg() as u32)
                .context("Failed to create spatial reference")?;
            dataset
                .set_spatial_ref(&srs)
                .context("Failed to set spatial reference")?;
            for (band_idx, (grid, nodata)) in [
                (&rasters.dsm, f64::NAN),
                (&rasters.dtm, f64::NAN),
                (&rasters.chm, 0.0f64),
            ]
            .iter()
            .enumerate()
            {
                let mut band = dataset
                    .rasterband(band_idx + 1)
                    .context(format!("Failed to get band {}", band_idx + 1))?;
                let data: Vec<f64> = grid
                    .iter()
                    .flat_map(|row| {
                        row.iter().map(|&val| {
                            if val == f64::NEG_INFINITY {
                                *nodata
                            } else {
                                val
                            }
                        })
                    })
                    .collect();
                let mut buffer = Buffer::new((rasters.width, rasters.height), data);
                band.write((0, 0), (rasters.width, rasters.height), &mut buffer)
                    .context(format!("Failed to write band {}", band_idx + 1))?;
                band.set_no_data_value(Some(*nodata))
                    .context(format!("Failed to set nodata for band {}", band_idx + 1))?;
            }
            println!("GeoTIFF saved to: {:?}", output_path);
        }
        Ok(output_path.to_path_buf())
    }

    pub fn run(
        &mut self,
        file_name: Option<String>,
        classification_list: Option<Vec<u8>>,
        resolution: Option<f64>,
        write_out_file: bool,
    ) -> Result<PathBuf> {
        let resolution = resolution.unwrap_or(1.0);
        self.ensure_points_loaded()?;
        let (min_x, min_y, max_x, max_y) = self.bbox_lambert93()?;
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded")?
            .clone();
        let rasters = self.process_lidar_points(
            points,
            (min_x, min_y, max_x, max_y),
            classification_list,
            resolution,
        )?;
        let output_file = self
            .output_path
            .join(file_name.unwrap_or("lidar_cdsm.tif".to_string()));
        let output_path = self.to_tif(&rasters, &output_file, write_out_file)?;
        Ok(output_path)
    }

    #[cfg(feature = "las")]
    pub fn save_las(&mut self, path: &Path) -> Result<PathBuf> {
        self.ensure_points_loaded()?;
        let points = self
            .loaded_points
            .as_ref()
            .context("No LiDAR points loaded. Call set_bbox() then run() or save().")?;
        if points.is_empty() {
            anyhow::bail!("No LiDAR points to export.");
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
        // Format 6 (LAS 1.4 extended): full u8 classification — required for IGN LiDAR HD
        // codes 64/66/67. Format 0 only allows 0–31 and rejects those classes.
        let mut builder = las::Builder::from((1, 4));
        builder.point_format = las::point::Format::new(6).context("Invalid point format")?;
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
                gps_time: Some(0.0), // required by point format 6
                ..Default::default()
            };
            writer
                .write_point(las_point)
                .map_err(|e| anyhow::anyhow!("Failed to write LAS point: {}", e))?;
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
        assert!(candidates.len() < 100);
        for &idx in &candidates {
            let p = &points[idx];
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
        let classes: Vec<u8> = reader
            .points()
            .map(|p| classification_to_u8(&p.unwrap().classification))
            .collect();
        assert_eq!(classes, vec![67, 64, 6]);
    }
}
