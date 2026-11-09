// Decode DSM, fill DTM holes, compute CHM — one thread per cell.
// mode 0 = legacy single 3x3 min pass (0.0 when no neighbour)
// mode 1 = nearest ground cell (seeds from Jump Flooding)

struct Grid {
    width: u32,
    height: u32,
    step: u32,
    mode: u32,
}

const EMPTY: u32 = 0x007fffffu;
const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> grid: Grid;
@group(0) @binding(1) var<storage, read> dsm: array<u32>;
@group(0) @binding(2) var<storage, read> dtm: array<u32>;
@group(0) @binding(3) var<storage, read> seeds: array<u32>;
@group(0) @binding(4) var<storage, read_write> out_dsm: array<f32>;
@group(0) @binding(5) var<storage, read_write> out_dtm: array<f32>;
@group(0) @binding(6) var<storage, read_write> out_chm: array<f32>;

fn decode(u: u32) -> f32 {
    if ((u & 0x80000000u) != 0u) {
        return bitcast<f32>(u & 0x7fffffffu);
    }
    return bitcast<f32>(~u);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= grid.width || gid.y >= grid.height) {
        return;
    }
    let idx = gid.y * grid.width + gid.x;

    let ds = dsm[idx];
    let dsm_empty = ds == EMPTY;
    let dsm_v = decode(ds);

    var dtm_v = 0.0;
    let own = dtm[idx];
    if (own != EMPTY) {
        dtm_v = decode(own);
    } else if (grid.mode == 1u) {
        let s = seeds[idx];
        if (s != NONE) {
            dtm_v = decode(dtm[s]);
        }
    } else {
        var found = false;
        var m = 0.0;
        let x = i32(gid.x);
        let y = i32(gid.y);
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let nx = x + dx;
                let ny = y + dy;
                if (nx < 0 || ny < 0 || nx >= i32(grid.width) || ny >= i32(grid.height)) {
                    continue;
                }
                let v = dtm[u32(ny) * grid.width + u32(nx)];
                if (v != EMPTY) {
                    let f = decode(v);
                    if (!found || f < m) {
                        m = f;
                        found = true;
                    }
                }
            }
        }
        dtm_v = m; // 0.0 if nothing found (legacy behaviour)
    }

    out_dsm[idx] = select(dsm_v, bitcast<f32>(0xff800000u), dsm_empty);
    out_dtm[idx] = dtm_v;
    out_chm[idx] = select(max(dsm_v - dtm_v, 0.0), 0.0, dsm_empty);
}
