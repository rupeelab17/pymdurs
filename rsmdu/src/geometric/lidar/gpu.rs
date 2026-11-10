//! Optional wgpu compute path (feature = "gpu"): max-Z binning, DTM hole filling, CHM.
//!
//! Rasters stay on the device between kernels; a single readback at the end.
//!
//! 1. `lidar_bin`       — DSM max-Z (enabled classes) + ground max-Z (class 2), uploaded in
//!                        batches that respect the device limits (binding size, dispatch size).
//! 2. `lidar_jfa_*`     — nearest ground cell by Jump Flooding (only for [`DtmFill::Nearest`]).
//! 3. `lidar_fill_chm`  — decode DSM, fill DTM holes, CHM = max(DSM − DTM, 0).
//!
//! Every failure (no adapter, raster too large, validation / OOM error) returns `Err`, so the
//! caller can fall back to the CPU path in `raster.rs`, which computes the same results.

use anyhow::{bail, Context, Result};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;

use super::raster::{jfa_steps, ClassMask};
use super::types::{DtmFill, LidarPoint};

pub(super) const GPU_MIN_POINTS: usize = 500_000;
/// Upper bound per upload batch (4 Mi points = 64 MiB); device limits may lower it.
pub(super) const GPU_BATCH_POINTS: usize = 1 << 22;

const BIN_WORKGROUP: u32 = 256;
const GRID_WORKGROUP: u32 = 16;
/// `f32_to_sortable(-inf)`: marker of an empty cell in the binned rasters.
const EMPTY_BITS: u32 = 0x007f_ffff;
/// JFA distances are computed in i32/u32 on the GPU: keep dx² + dy² below 2³¹.
const JFA_MAX_DIM: usize = 32_768;

static GPU_LOCK: Mutex<()> = Mutex::new(());

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuLidarPoint {
    /// X − x_min
    x: f32,
    /// y_max − Y
    y: f32,
    z: f32,
    classification: u32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BinParams {
    resolution: f32,
    width: u32,
    height: u32,
    n_points: u32,
    class_mask: [u32; 8],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GridParams {
    width: u32,
    height: u32,
    step: u32,
    mode: u32,
}

/// Row-major rasters (`row * width + col`). Empty DSM cells are `-inf`.
pub(super) struct GpuRasters {
    pub(super) dsm: Vec<f32>,
    pub(super) dtm: Vec<f32>,
    pub(super) chm: Vec<f32>,
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bin: wgpu::ComputePipeline,
    jfa_init: wgpu::ComputePipeline,
    jfa_step: wgpu::ComputePipeline,
    fill_chm: wgpu::ComputePipeline,
}

fn make_pipeline(device: &wgpu::Device, label: &str, wgsl: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None, // derived from the shader: one bind group, bindings 0..n in order
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}

impl GpuContext {
    async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("Failed to find GPU adapter: {e:?}"))?;

        // Raise only the limits that bound point batches and raster size.
        // `Limits::default()` caps a storage binding at 128 MiB (~8.4 M points).
        let adapter_limits = adapter.limits();
        let required_limits = wgpu::Limits {
            max_storage_buffer_binding_size: adapter_limits.max_storage_buffer_binding_size,
            max_buffer_size: adapter_limits.max_buffer_size,
            ..wgpu::Limits::default()
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Lidar raster compute"),
                required_features: wgpu::Features::empty(),
                required_limits,
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
            .map_err(|e| format!("Failed to create GPU device: {e}"))?;

        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bin = make_pipeline(&device, "lidar_bin", include_str!("shaders/lidar_bin.wgsl"));
        let jfa_init = make_pipeline(
            &device,
            "lidar_jfa_init",
            include_str!("shaders/lidar_jfa_init.wgsl"),
        );
        let jfa_step = make_pipeline(
            &device,
            "lidar_jfa_step",
            include_str!("shaders/lidar_jfa_step.wgsl"),
        );
        let fill_chm = make_pipeline(
            &device,
            "lidar_fill_chm",
            include_str!("shaders/lidar_fill_chm.wgsl"),
        );
        if let Some(e) = device.pop_error_scope().await {
            return Err(format!("Lidar WGSL pipelines failed validation: {e}"));
        }

        Ok(Self {
            device,
            queue,
            bin,
            jfa_init,
            jfa_step,
            fill_chm,
        })
    }

    fn bind(
        &self,
        pipeline: &wgpu::ComputePipeline,
        label: &str,
        buffers: &[&wgpu::Buffer],
    ) -> wgpu::BindGroup {
        let layout = pipeline.get_bind_group_layout(0);
        let entries: Vec<wgpu::BindGroupEntry> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &layout,
            entries: &entries,
        })
    }

    fn grid_uniform(&self, width: u32, height: u32, step: u32, mode: u32) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lidar_grid_params"),
                contents: bytemuck::bytes_of(&GridParams {
                    width,
                    height,
                    step,
                    mode,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            })
    }

    fn storage(&self, label: &str, bytes: u64, extra: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | extra,
            mapped_at_creation: false,
        })
    }
}

fn gpu_context() -> Result<&'static GpuContext, String> {
    // Only cache Ok — a failed init must be retryable (adapter may appear later).
    static CTX: OnceLock<GpuContext> = OnceLock::new();
    if let Some(ctx) = CTX.get() {
        return Ok(ctx);
    }
    let ctx = pollster::block_on(GpuContext::new())?;
    Ok(CTX.get_or_init(|| ctx))
}

fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    groups: (u32, u32),
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some(label),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.dispatch_workgroups(groups.0, groups.1, 1);
}

fn read_f32(buffer: &wgpu::Buffer) -> Vec<f32> {
    let out = {
        let view = buffer.slice(..).get_mapped_range();
        bytemuck::cast_slice::<u8, f32>(&view).to_vec()
    };
    buffer.unmap();
    out
}

/// DSM / filled DTM / CHM on GPU. `Err` → caller falls back to CPU.
pub(super) fn rasterize_fill_chm_gpu(
    points: &[LidarPoint],
    bbox: (f64, f64, f64, f64),
    mask: &ClassMask,
    resolution: f64,
    width: usize,
    height: usize,
    fill: DtmFill,
) -> Result<GpuRasters> {
    rasterize_fill_chm_gpu_batched(
        points,
        bbox,
        mask,
        resolution,
        width,
        height,
        fill,
        GPU_BATCH_POINTS,
    )
}

/// Same as [`rasterize_fill_chm_gpu`] with an explicit batch size (tests force several batches).
#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize_fill_chm_gpu_batched(
    points: &[LidarPoint],
    bbox: (f64, f64, f64, f64),
    mask: &ClassMask,
    resolution: f64,
    width: usize,
    height: usize,
    fill: DtmFill,
    max_batch_points: usize,
) -> Result<GpuRasters> {
    let cells = width
        .checked_mul(height)
        .context("raster dimensions overflow")?;
    if cells == 0 {
        return Ok(GpuRasters {
            dsm: Vec::new(),
            dtm: Vec::new(),
            chm: Vec::new(),
        });
    }
    let w32 = u32::try_from(width).context("raster width exceeds u32")?;
    let h32 = u32::try_from(height).context("raster height exceeds u32")?;
    if fill == DtmFill::Nearest && width.max(height) > JFA_MAX_DIM {
        bail!("raster {width}x{height} too large for GPU JFA (max side {JFA_MAX_DIM})");
    }

    let ctx = gpu_context().map_err(|e| anyhow::anyhow!(e))?;
    // Error scopes are per device: serialize concurrent callers.
    let _guard = GPU_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    // ---- Limits ---------------------------------------------------------------------------
    let limits = ctx.device.limits();
    let max_binding = limits.max_storage_buffer_binding_size as u64;
    let max_buffer = limits.max_buffer_size as u64;
    let max_groups = limits.max_compute_workgroups_per_dimension;

    let grid_bytes = (cells as u64) * 4;
    if grid_bytes > max_binding || grid_bytes > max_buffer {
        bail!(
            "raster {width}x{height} ({grid_bytes} B per band) exceeds GPU buffer limits \
             (binding {max_binding} B, buffer {max_buffer} B)"
        );
    }
    let groups_x = w32.div_ceil(GRID_WORKGROUP);
    let groups_y = h32.div_ceil(GRID_WORKGROUP);
    if groups_x > max_groups || groups_y > max_groups {
        bail!("raster {width}x{height} exceeds GPU dispatch limits ({max_groups} groups/dim)");
    }

    let point_size = std::mem::size_of::<GpuLidarPoint>() as u64;
    let batch_cap = (max_batch_points.max(1) as u64)
        .min(max_binding / point_size)
        .min(max_buffer / point_size)
        .min(max_groups as u64 * BIN_WORKGROUP as u64)
        .min(points.len().max(1) as u64) as usize;

    let mode = match fill {
        DtmFill::Neighbor3x3 => 0u32,
        DtmFill::Nearest => 1u32,
    };

    ctx.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);

    // ---- 1. Binning, batch by batch ------------------------------------------------------
    let empty = vec![EMPTY_BITS; cells];
    let init_grid = |label: &str| {
        ctx.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&empty),
                usage: wgpu::BufferUsages::STORAGE,
            })
    };
    let dsm_bits = init_grid("lidar_dsm_bits");
    let dtm_bits = init_grid("lidar_dtm_bits");
    drop(empty);

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_bin_params"),
        size: std::mem::size_of::<BinParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let points_buf = ctx.storage(
        "lidar_points",
        batch_cap as u64 * point_size,
        wgpu::BufferUsages::COPY_DST,
    );
    let bin_bg = ctx.bind(
        &ctx.bin,
        "lidar_bin_bg",
        &[&params_buf, &points_buf, &dsm_bits, &dtm_bits],
    );

    let (x_min, _y_min, _x_max, y_max) = bbox;
    let mut batch: Vec<GpuLidarPoint> = Vec::with_capacity(batch_cap);
    for chunk in points.chunks(batch_cap) {
        // Local origin before f32: Lambert-93 absolutes lose ~0.5 m in f32.
        batch.clear();
        batch.extend(chunk.iter().map(|p| GpuLidarPoint {
            x: (p.x - x_min) as f32,
            y: (y_max - p.y) as f32,
            z: p.z as f32,
            classification: p.classification as u32,
        }));
        let params = BinParams {
            resolution: resolution as f32,
            width: w32,
            height: h32,
            n_points: chunk.len() as u32,
            class_mask: mask.words(),
        };
        // write_buffer takes effect at the next submit, after previously submitted work:
        // reusing the same points buffer across batches is safe, and the CPU converts the
        // next batch while the GPU bins the current one.
        ctx.queue
            .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));
        ctx.queue
            .write_buffer(&points_buf, 0, bytemuck::cast_slice(&batch));
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lidar_bin_enc"),
            });
        dispatch(
            &mut encoder,
            "lidar_bin_pass",
            &ctx.bin,
            &bin_bg,
            ((chunk.len() as u32).div_ceil(BIN_WORKGROUP), 1),
        );
        ctx.queue.submit(Some(encoder.finish()));
    }
    drop(batch);

    // ---- 2. Nearest ground seeds (JFA) ----------------------------------------------------
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lidar_fill_enc"),
        });
    let seeds: Option<wgpu::Buffer> = if mode == 1 {
        let mut src = ctx.storage("lidar_seeds_a", grid_bytes, wgpu::BufferUsages::empty());
        let mut dst = ctx.storage("lidar_seeds_b", grid_bytes, wgpu::BufferUsages::empty());
        let bg = ctx.bind(
            &ctx.jfa_init,
            "lidar_jfa_init_bg",
            &[&ctx.grid_uniform(w32, h32, 0, mode), &dtm_bits, &src],
        );
        dispatch(&mut encoder, "lidar_jfa_init", &ctx.jfa_init, &bg, (groups_x, groups_y));
        for step in jfa_steps(width, height) {
            let bg = ctx.bind(
                &ctx.jfa_step,
                "lidar_jfa_step_bg",
                &[&ctx.grid_uniform(w32, h32, step, mode), &src, &dst],
            );
            dispatch(&mut encoder, "lidar_jfa_step", &ctx.jfa_step, &bg, (groups_x, groups_y));
            std::mem::swap(&mut src, &mut dst);
        }
        Some(src)
    } else {
        None
    };
    // Mode 0 never reads the seeds: bind the (read-only) DTM there instead of a dummy buffer.
    let seeds_ref = seeds.as_ref().unwrap_or(&dtm_bits);

    // ---- 3. Fill + CHM, then readback ----------------------------------------------------
    let outputs: [wgpu::Buffer; 3] = std::array::from_fn(|i| {
        ctx.storage(
            ["lidar_out_dsm", "lidar_out_dtm", "lidar_out_chm"][i],
            grid_bytes,
            wgpu::BufferUsages::COPY_SRC,
        )
    });
    let fill_bg = ctx.bind(
        &ctx.fill_chm,
        "lidar_fill_chm_bg",
        &[
            &ctx.grid_uniform(w32, h32, 0, mode),
            &dsm_bits,
            &dtm_bits,
            seeds_ref,
            &outputs[0],
            &outputs[1],
            &outputs[2],
        ],
    );
    dispatch(&mut encoder, "lidar_fill_chm", &ctx.fill_chm, &fill_bg, (groups_x, groups_y));

    let staging: [wgpu::Buffer; 3] = std::array::from_fn(|_| {
        ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lidar_staging"),
            size: grid_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    for (src, dst) in outputs.iter().zip(&staging) {
        encoder.copy_buffer_to_buffer(src, 0, dst, 0, grid_bytes);
    }
    let submission_index = ctx.queue.submit(Some(encoder.finish()));

    let validation = pollster::block_on(ctx.device.pop_error_scope());
    let oom = pollster::block_on(ctx.device.pop_error_scope());
    if let Some(e) = validation.or(oom) {
        bail!("GPU raster pipeline error: {e}");
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    for buffer in &staging {
        let sender = sender.clone();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
    }
    drop(sender);
    ctx.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: Some(std::time::Duration::from_secs(120)),
        })
        .map_err(|e| anyhow::anyhow!("GPU poll failed: {e:?}"))?;
    for _ in 0..staging.len() {
        receiver
            .recv()
            .context("GPU map channel closed")?
            .map_err(|e| anyhow::anyhow!("Failed to map GPU staging buffer: {e:?}"))?;
    }

    Ok(GpuRasters {
        dsm: read_f32(&staging[0]),
        dtm: read_f32(&staging[1]),
        chm: read_f32(&staging[2]),
    })
}
