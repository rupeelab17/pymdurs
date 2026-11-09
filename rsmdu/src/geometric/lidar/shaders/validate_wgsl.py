"""Usage : uv run --with wgpu --with numpy python validate_wgsl.py (GPU ou lavapipe/llvmpipe).
   Exécute les kernels WGSL (même enchaînement que gpu.rs) sur llvmpipe et compare à une référence NumPy."""
import numpy as np, wgpu, struct, time, sys
import os
SH = os.path.dirname(os.path.abspath(__file__)) + "/"
EMPTY = 0x007fffff; NONE = 0xffffffff
S = wgpu.BufferUsage.STORAGE; U = wgpu.BufferUsage.UNIFORM
CD = wgpu.BufferUsage.COPY_DST; CS = wgpu.BufferUsage.COPY_SRC

adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
dev = adapter.request_device_sync()
def pipe(name):
    m = dev.create_shader_module(code=open(SH + name).read())
    return dev.create_compute_pipeline(layout="auto", compute={"module": m, "entry_point": "main"})
P_BIN, P_INIT, P_STEP, P_FILL = (pipe(n) for n in
    ["lidar_bin.wgsl", "lidar_jfa_init.wgsl", "lidar_jfa_step.wgsl", "lidar_fill_chm.wgsl"])
def bg(p, bufs):
    return dev.create_bind_group(layout=p.get_bind_group_layout(0),
        entries=[{"binding": i, "resource": {"buffer": b, "offset": 0, "size": b.size}} for i, b in enumerate(bufs)])
def grid_u(w, h, step, mode):
    return dev.create_buffer_with_data(data=struct.pack("<4I", w, h, step, mode), usage=U)

def mask_words(classes):
    m = np.zeros(8, np.uint32)
    for c in (range(256) if classes is None else classes):
        m[c >> 5] |= np.uint32(1 << (c & 31))
    return m

def jfa_steps(w, h):
    n = 1
    while n < max(w, h): n <<= 1
    k = n // 2; steps = []
    while k >= 1: steps.append(k); k //= 2
    return steps + [1]

def gpu_run(x, y, z, cls, res, w, h, classes, mode, batch_cap):
    cells = w * h
    dsm = dev.create_buffer_with_data(data=np.full(cells, EMPTY, np.uint32), usage=S | CS)
    dtm = dev.create_buffer_with_data(data=np.full(cells, EMPTY, np.uint32), usage=S | CS)
    params = dev.create_buffer(size=48, usage=U | CD)
    pts = dev.create_buffer(size=batch_cap * 16, usage=S | CD)
    g_bin = bg(P_BIN, [params, pts, dsm, dtm])
    mw = mask_words(classes)
    n = len(x)
    for s in range(0, n, batch_cap):          # une soumission par lot
        e = min(n, s + batch_cap)
        rec = np.zeros((e - s, 4), np.float32)
        rec[:, 0], rec[:, 1], rec[:, 2] = x[s:e], y[s:e], z[s:e]
        rec[:, 3] = cls[s:e].astype(np.uint32).view(np.float32)
        dev.queue.write_buffer(params, 0, struct.pack("<f3I", res, w, h, e - s) + mw.tobytes())
        dev.queue.write_buffer(pts, 0, rec.tobytes())
        enc = dev.create_command_encoder(); cp = enc.begin_compute_pass()
        cp.set_pipeline(P_BIN); cp.set_bind_group(0, g_bin)
        cp.dispatch_workgroups((e - s + 255) // 256); cp.end()
        dev.queue.submit([enc.finish()])

    gx, gy = (w + 15) // 16, (h + 15) // 16
    enc = dev.create_command_encoder()
    seeds_final = dtm  # mode 0 : on lie dtm à la place des seeds
    if mode == 1:
        a = dev.create_buffer(size=cells * 4, usage=S); b = dev.create_buffer(size=cells * 4, usage=S)
        cp = enc.begin_compute_pass(); cp.set_pipeline(P_INIT)
        cp.set_bind_group(0, bg(P_INIT, [grid_u(w, h, 0, 1), dtm, a])); cp.dispatch_workgroups(gx, gy); cp.end()
        for k in jfa_steps(w, h):
            cp = enc.begin_compute_pass(); cp.set_pipeline(P_STEP)
            cp.set_bind_group(0, bg(P_STEP, [grid_u(w, h, k, 1), a, b])); cp.dispatch_workgroups(gx, gy); cp.end()
            a, b = b, a
        seeds_final = a
    o = [dev.create_buffer(size=cells * 4, usage=S | CS) for _ in range(3)]
    cp = enc.begin_compute_pass(); cp.set_pipeline(P_FILL)
    cp.set_bind_group(0, bg(P_FILL, [grid_u(w, h, 0, mode), dsm, dtm, seeds_final] + o))
    cp.dispatch_workgroups(gx, gy); cp.end()
    dev.queue.submit([enc.finish()])
    return [np.frombuffer(dev.queue.read_buffer(b), np.float32).reshape(h, w) for b in o]

# ---------------- référence CPU (même sémantique) ----------------
def cpu_run(x, y, z, cls, res, w, h, classes, mode):
    keep = np.ones(len(x), bool) if classes is None else np.isin(cls, classes)
    col = np.floor(x / res).astype(np.int64); row = np.floor(y / res).astype(np.int64)
    keep &= (x >= 0) & (y >= 0) & (col < w) & (row < h)
    dsm = np.full(w * h, -np.inf, np.float32); dtm = np.full(w * h, -np.inf, np.float32)
    idx = (row * w + col)[keep]
    np.maximum.at(dsm, idx, z[keep])
    g = keep & (cls == 2)
    np.maximum.at(dtm, (row * w + col)[g], z[g])
    dsm = dsm.reshape(h, w); dtm = dtm.reshape(h, w)
    filled = dtm.copy(); empty = ~np.isfinite(dtm)
    if mode == 0:
        pad = np.pad(dtm, 1, constant_values=np.inf); pad[~np.isfinite(pad)] = np.inf
        m = np.min(np.stack([pad[1+dy:1+dy+h, 1+dx:1+dx+w] for dy in (-1,0,1) for dx in (-1,0,1)]), 0)
        filled[empty] = np.where(np.isfinite(m), m, 0.0)[empty]
    else:
        seeds = np.where(empty, NONE, np.arange(w * h).reshape(h, w)).astype(np.int64)
        yy, xx = np.mgrid[0:h, 0:w]
        for k in jfa_steps(w, h):
            best = np.full((h, w), NONE, np.int64); bd = np.full((h, w), 2**32 - 1, np.int64)
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    ny, nx = yy + dy * k, xx + dx * k
                    ok = (nx >= 0) & (ny >= 0) & (nx < w) & (ny < h)
                    s = np.full((h, w), NONE, np.int64); s[ok] = seeds[ny[ok], nx[ok]]
                    ok &= s != NONE
                    d = (s % w - xx) ** 2 + (s // w - yy) ** 2
                    better = ok & ((d < bd) | ((d == bd) & (s < best)))
                    best[better] = s[better]; bd[better] = d[better]
            seeds = best
        flat = dtm.ravel()
        filled[empty] = np.where(seeds[empty] == NONE, 0.0, flat[np.where(seeds == NONE, 0, seeds)][empty])
    chm = np.where(np.isfinite(dsm), np.maximum(dsm - filled, 0), 0).astype(np.float32)
    return dsm, filled, chm

rng = np.random.default_rng(0)
def case(name, n, w, h, res, classes, mode, batch_cap, building=True):
    # points aux centres de cellules ± 0.4·res pour éviter les ambiguïtés d'arrondi en bordure
    cx = rng.integers(0, w, n); cy = rng.integers(0, h, n)
    x = ((cx + 0.5 + rng.uniform(-.4, .4, n)) * res).astype(np.float32)
    y = ((cy + 0.5 + rng.uniform(-.4, .4, n)) * res).astype(np.float32)
    z = rng.uniform(10, 30, n).astype(np.float32)
    cls = rng.choice([1, 2, 3, 5, 6, 7, 66], n).astype(np.uint32)
    if building:  # grand bâtiment : aucun point sol dessous
        inb = (cx > w // 4) & (cx < w // 2) & (cy > h // 4) & (cy < 3 * h // 4)
        cls[inb & (cls == 2)] = 6
    t = time.time(); G = gpu_run(x, y, z, cls, res, w, h, classes, mode, batch_cap); tg = time.time() - t
    C = cpu_run(x, y, z, cls, res, w, h, classes, mode)
    ok = True
    for lab, g, c in zip(["dsm", "dtm", "chm"], G, C):
        same_inf = np.array_equal(np.isinf(g), np.isinf(c))
        fin = np.isfinite(c)
        err = np.max(np.abs(g[fin] - c[fin])) if fin.any() else 0
        ok &= same_inf and err < 1e-4
        print(f"  {lab}: max|gpu-cpu|={err:.2e} inf_match={same_inf}")
    zeros = int(np.sum(G[1] == 0.0))
    print(f"{'OK ' if ok else 'ÉCHEC'} {name} (gpu {tg:.2f}s, cellules DTM=0.0 : {zeros})")
    return ok

res = []
res.append(case("mode 0 (legacy 3x3), toutes classes, 1 lot", 200_000, 300, 200, 1.0, None, 0, 1 << 20))
res.append(case("mode 0, filtre classes [2,6], 7 lots", 200_000, 300, 200, 1.0, [2, 6], 0, 30_000))
res.append(case("mode 1 (JFA plus proche), 5 lots", 200_000, 300, 200, 1.0, None, 1, 50_000))
res.append(case("mode 1, grille non carrée 517x129, rés. 0.5", 150_000, 517, 129, 0.5, [1, 2, 66], 1, 40_000))
res.append(case("mode 1, sans sol du tout", 5_000, 64, 64, 1.0, [1, 6], 1, 4096, building=False))
sys.exit(0 if all(res) else 1)
