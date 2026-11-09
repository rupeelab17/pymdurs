//! Native COPC 1.0 reader (hierarchy + LAZ chunks).

use anyhow::Result;
use laz::laszip::LazVlr;

use super::types::LidarPoint;

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
pub(super) struct Las14Header {
    /// Format de point (bits 7-6 masqués = flag LAZ)
    pub(super) point_format: u8,
    /// Taille d'un enregistrement de point en bytes
    pub(super) point_record_len: u16,
    /// Offset vers les données de points (bytes 96-99, u32 LE)
    #[allow(dead_code)]
    pub(super) offset_to_point_data: u64,
    /// Nombre de VLRs (bytes 100-103)
    pub(super) vlr_count: u32,
    /// Échelles XYZ (bytes 131/139/147, f64 LE)
    pub(super) scale: [f64; 3],
    /// Offsets de coordonnées XYZ (bytes 155/163/171, f64 LE)
    pub(super) coord_offset: [f64; 3],
    /// Offset vers le premier EVLR (bytes 235-242, u64 LE)
    #[allow(dead_code)]
    pub(super) evlr_offset: u64,
    /// Nombre de points 64-bit LAS 1.4 (bytes 247-254, u64 LE)
    pub(super) point_count: u64,
}

/// COPC Info VLR — 160 bytes de données (offset fichier 429).
#[derive(Debug, Clone)]
pub(super) struct CopcInfo {
    pub(super) center_x: f64,
    pub(super) center_y: f64,
    pub(super) center_z: f64,
    pub(super) halfsize: f64,
    #[allow(dead_code)]
    pub(super) spacing: f64,
    /// Offset absolu dans le fichier vers la page racine de hiérarchie
    pub(super) root_hier_offset: u64,
    /// Taille en bytes de la page racine
    pub(super) root_hier_size: u64,
}

/// Une entrée dans la hiérarchie COPC (32 bytes).
#[derive(Debug, Clone)]
pub(super) struct CopcEntry {
    /// Niveau de l'octree (0 = racine)
    pub(super) level: i32,
    /// Coordonnées de voxel X
    pub(super) vx: i32,
    /// Coordonnées de voxel Y
    pub(super) vy: i32,
    #[allow(dead_code)]
    pub(super) vz: i32,
    /// Offset absolu dans le fichier vers le chunk LAZ compressé
    pub(super) offset: u64,
    /// Taille compressée en bytes ; si -1 = page enfant de hiérarchie
    pub(super) byte_size: i32,
    /// Nombre de points ; -1 = page enfant ; 0 = nœud vide
    pub(super) point_count: i32,
}

// --- helpers binaires little-endian (pas de panic sur buffer tronqué) ---
#[inline]
fn le_bytes<const N: usize>(b: &[u8], o: usize) -> Result<[u8; N]> {
    b.get(o..o + N)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("buffer trop court @ {o}+{N} (len={})", b.len()))
}

#[inline]
pub(super) fn ru16(b: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(le_bytes(b, o)?))
}
#[inline]
pub(super) fn ru32(b: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(le_bytes(b, o)?))
}
#[inline]
pub(super) fn ru64(b: &[u8], o: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(le_bytes(b, o)?))
}
#[inline]
pub(super) fn ri32(b: &[u8], o: usize) -> Result<i32> {
    Ok(i32::from_le_bytes(le_bytes(b, o)?))
}
#[inline]
pub(super) fn rf64(b: &[u8], o: usize) -> Result<f64> {
    Ok(f64::from_le_bytes(le_bytes(b, o)?))
}

/// Parse le LAS 1.4 public header depuis un buffer ≥ 375 bytes.
pub(super) fn parse_las14_header(buf: &[u8]) -> Result<Las14Header> {
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
        point_record_len: ru16(buf, 105)?,
        offset_to_point_data: ru32(buf, 96)? as u64,
        vlr_count: ru32(buf, 100)?,
        scale: [rf64(buf, 131)?, rf64(buf, 139)?, rf64(buf, 147)?],
        coord_offset: [rf64(buf, 155)?, rf64(buf, 163)?, rf64(buf, 171)?],
        evlr_offset: ru64(buf, 235)?,
        point_count: ru64(buf, 247)?,
    })
}

/// Parse le COPC Info VLR depuis 160 bytes (data seule, sans les 54 bytes de VLR header).
pub(super) fn parse_copc_info(buf: &[u8]) -> Result<CopcInfo> {
    if buf.len() < 160 {
        anyhow::bail!("COPC Info VLR trop court: {} bytes", buf.len());
    }
    Ok(CopcInfo {
        center_x: rf64(buf, 0)?,
        center_y: rf64(buf, 8)?,
        center_z: rf64(buf, 16)?,
        halfsize: rf64(buf, 24)?,
        spacing: rf64(buf, 32)?,
        root_hier_offset: ru64(buf, 40)?,
        root_hier_size: ru64(buf, 48)?,
    })
}

/// Parse une page de hiérarchie COPC (entrées de 32 bytes chacune).
pub(super) fn parse_hierarchy_page(buf: &[u8]) -> Result<Vec<CopcEntry>> {
    let n = buf.len() / 32;
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        let b = i * 32;
        entries.push(CopcEntry {
            level: ri32(buf, b)?,
            vx: ri32(buf, b + 4)?,
            vy: ri32(buf, b + 8)?,
            vz: ri32(buf, b + 12)?,
            offset: ru64(buf, b + 16)?,
            byte_size: ri32(buf, b + 24)?,
            point_count: ri32(buf, b + 28)?,
        });
    }
    Ok(entries)
}

/// Lit toutes les entrées de hiérarchie COPC en suivant les pages enfants.
/// Les entrées avec point_count == -1 sont des pages enfants à lire récursivement.
pub(super) fn read_all_hierarchy_entries(file_buf: &[u8], info: &CopcInfo) -> Result<Vec<CopcEntry>> {
    let mut all_entries = Vec::new();
    let mut pages_to_read: Vec<(u64, u64)> = vec![(info.root_hier_offset, info.root_hier_size)];

    while let Some((page_offset, page_size)) = pages_to_read.pop() {
        let start = page_offset as usize;
        let end = start + page_size as usize;
        if end > file_buf.len() {
            tracing::warn!(
                "  ⚠️  Page hiérarchie COPC hors limites: offset={} size={} file_len={}",
                page_offset,
                page_size,
                file_buf.len()
            );
            continue;
        }
        for entry in parse_hierarchy_page(&file_buf[start..end])? {
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
pub(super) fn copc_entry_bounds_xy(entry: &CopcEntry, info: &CopcInfo) -> (f64, f64, f64, f64) {
    let side_size = (2.0 * info.halfsize) / (1u64 << entry.level.max(0)) as f64;
    let root_min_x = info.center_x - info.halfsize;
    let root_min_y = info.center_y - info.halfsize;
    let min_x = root_min_x + entry.vx as f64 * side_size;
    let min_y = root_min_y + entry.vy as f64 * side_size;
    (min_x, min_y, min_x + side_size, min_y + side_size)
}

/// Teste si un nœud COPC intersecte le bbox de requête (XY uniquement).
pub(super) fn copc_entry_intersects(
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
pub(super) fn find_laz_vlr(file_buf: &[u8], vlr_count: u32) -> Result<LazVlr> {
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
        let record_id = ru16(file_buf, pos + 18)?;
        let record_len = ru16(file_buf, pos + 20)? as usize;
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
pub(super) fn decompress_copc_chunk(
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
pub(super) fn decode_las14_point(
    raw: &[u8],
    scale: &[f64; 3],
    coord_offset: &[f64; 3],
) -> Result<LidarPoint> {
    // Format 6+ : XYZ (12) + … + classification @ offset 16
    if raw.len() < 17 {
        anyhow::bail!("point LAS trop court: {} bytes (min 17)", raw.len());
    }
    let xi = i32::from_le_bytes(le_bytes(raw, 0)?);
    let yi = i32::from_le_bytes(le_bytes(raw, 4)?);
    let zi = i32::from_le_bytes(le_bytes(raw, 8)?);
    Ok(LidarPoint {
        x: xi as f64 * scale[0] + coord_offset[0],
        y: yi as f64 * scale[1] + coord_offset[1],
        z: zi as f64 * scale[2] + coord_offset[2],
        classification: raw[16],
    })
}

/// Statistiques de lecture COPC
pub(super) struct CopcReadStats {
    pub(super) points: Vec<LidarPoint>,
    pub(super) entries_processed: usize,
    pub(super) entries_success: usize,
    pub(super) entries_failed: usize,
}
