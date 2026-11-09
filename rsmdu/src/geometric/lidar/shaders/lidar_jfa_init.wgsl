// Jump Flooding seeds: every ground cell is its own seed, empty cells have none.

struct Grid {
    width: u32,
    height: u32,
    step: u32,
    mode: u32,
}

const EMPTY: u32 = 0x007fffffu; // f32_to_sortable(-inf)
const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> grid: Grid;
@group(0) @binding(1) var<storage, read> dtm: array<u32>;
@group(0) @binding(2) var<storage, read_write> seeds: array<u32>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= grid.width || gid.y >= grid.height) {
        return;
    }
    let idx = gid.y * grid.width + gid.x;
    seeds[idx] = select(idx, NONE, dtm[idx] == EMPTY);
}
