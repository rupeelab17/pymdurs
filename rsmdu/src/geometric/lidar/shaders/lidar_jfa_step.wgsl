// One Jump Flooding pass at offset `step`. Ties broken by smallest seed index,
// so the CPU reference (raster.rs) reproduces the result bit for bit.

struct Grid {
    width: u32,
    height: u32,
    step: u32,
    mode: u32,
}

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> grid: Grid;
@group(0) @binding(1) var<storage, read> seeds_in: array<u32>;
@group(0) @binding(2) var<storage, read_write> seeds_out: array<u32>;

fn dist2(s: u32, x: i32, y: i32) -> u32 {
    let dx = i32(s % grid.width) - x;
    let dy = i32(s / grid.width) - y;
    return u32(dx * dx + dy * dy);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= grid.width || gid.y >= grid.height) {
        return;
    }
    let x = i32(gid.x);
    let y = i32(gid.y);
    let k = i32(grid.step);
    let w = i32(grid.width);
    let h = i32(grid.height);
    var best = NONE;
    var best_d = 0xffffffffu;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let nx = x + dx * k;
            let ny = y + dy * k;
            if (nx < 0 || ny < 0 || nx >= w || ny >= h) {
                continue;
            }
            let s = seeds_in[u32(ny) * grid.width + u32(nx)];
            if (s == NONE) {
                continue;
            }
            let d = dist2(s, x, y);
            if (d < best_d || (d == best_d && s < best)) {
                best = s;
                best_d = d;
            }
        }
    }
    seeds_out[gid.y * grid.width + gid.x] = best;
}
