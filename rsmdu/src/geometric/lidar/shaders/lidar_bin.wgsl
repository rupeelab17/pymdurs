// Max-Z binning: DSM (all enabled classes) + ground DTM (class 2).
// Points arrive pre-shifted: x = X - x_min, y = y_max - Y (both >= 0 inside bbox).

struct BinParams {
    resolution: f32,
    width: u32,
    height: u32,
    n_points: u32,
    class_mask: array<vec4<u32>, 2>, // 256-bit class filter
}

struct Point {
    x: f32,
    y: f32,
    z: f32,
    classification: u32,
}

@group(0) @binding(0) var<uniform> params: BinParams;
@group(0) @binding(1) var<storage, read> points: array<Point>;
@group(0) @binding(2) var<storage, read_write> dsm: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> dtm: array<atomic<u32>>;

fn f32_to_sortable(f: f32) -> u32 {
    let bits = bitcast<u32>(f);
    if ((bits & 0x80000000u) != 0u) {
        return ~bits;
    }
    return bits | 0x80000000u;
}

fn class_enabled(c: u32) -> bool {
    let word = params.class_mask[c >> 7u][(c >> 5u) & 3u];
    return (word & (1u << (c & 31u))) != 0u;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.n_points) {
        return;
    }
    let p = points[i];
    let c = p.classification & 255u;
    if (!class_enabled(c)) {
        return;
    }
    // Explicit test: f32 -> u32 conversion saturates, so negatives would land in col/row 0.
    if (!(p.x >= 0.0) || !(p.y >= 0.0)) {
        return;
    }
    let col = u32(floor(p.x / params.resolution));
    let row = u32(floor(p.y / params.resolution));
    if (col >= params.width || row >= params.height) {
        return;
    }
    let idx = row * params.width + col;
    let zb = f32_to_sortable(p.z);
    atomicMax(&dsm[idx], zb);
    if (c == 2u) {
        atomicMax(&dtm[idx], zb);
    }
}
