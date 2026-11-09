//! Optional wgpu headless max-Z rasterization (feature = "gpu").

use anyhow::{Context, Result};

use super::types::LidarPoint;

#[cfg(feature = "gpu")]
pub(super) const GPU_MIN_POINTS: usize = 500_000;

#[cfg(feature = "gpu")]
pub(super) fn f32_to_sortable_u32(f: f32) -> u32 {
    let bits = f.to_bits();
    if bits & 0x8000_0000 != 0 {
        !bits
    } else {
        bits | 0x8000_0000
    }
}

#[cfg(feature = "gpu")]
pub(super) fn sortable_u32_to_f32(u: u32) -> f32 {
    if u & 0x8000_0000 != 0 {
        f32::from_bits(u & 0x7fff_ffff)
    } else {
        f32::from_bits(!u)
    }
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GpuLidarPoint {
    x: f32,
    y: f32,
    z: f32,
    classification: u32,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GpuRasterParams {
    x_min: f32,
    y_max: f32,
    resolution: f32,
    width: u32,
    height: u32,
    _pad: [u32; 3],
}

#[cfg(feature = "gpu")]
pub(super) struct GpuRasterContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

#[cfg(feature = "gpu")]
impl GpuRasterContext {
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

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Lidar max-Z compute"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                experimental_features: Default::default(),
                trace: Default::default(),
            })
            .await
            .map_err(|e| format!("Failed to create GPU device: {e}"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lidar_max_z"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
struct Params {
    x_min: f32,
    y_max: f32,
    resolution: f32,
    width: u32,
    height: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct Point {
    x: f32,
    y: f32,
    z: f32,
    classification: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
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

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&points)) {
        return;
    }
    let p = points[i];
    let col = u32(floor((p.x - params.x_min) / params.resolution));
    let row = u32(floor((params.y_max - p.y) / params.resolution));
    if (col >= params.width || row >= params.height) {
        return;
    }
    let idx = row * params.width + col;
    let z_bits = f32_to_sortable(p.z);
    atomicMax(&dsm[idx], z_bits);
    if (p.classification == 2u) {
        atomicMax(&dtm[idx], z_bits);
    }
}
"#
                .into(),
            ),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("lidar_max_z_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lidar_max_z_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("lidar_max_z_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
        })
    }
}

#[cfg(feature = "gpu")]
pub(super) fn gpu_raster_context() -> Result<&'static GpuRasterContext, String> {
    use std::sync::OnceLock;
    static CTX: OnceLock<Result<GpuRasterContext, String>> = OnceLock::new();
    match CTX.get_or_init(|| pollster::block_on(GpuRasterContext::new())) {
        Ok(ctx) => Ok(ctx),
        Err(e) => Err(e.clone()),
    }
}

/// Bin max-Z DSM (+ DTM class 2) on GPU. Err → caller falls back to CPU.
#[cfg(feature = "gpu")]
pub(super) fn rasterize_max_z_gpu(
    points: &[LidarPoint],
    bbox: (f64, f64, f64, f64),
    resolution: f64,
    width: usize,
    height: usize,
) -> Result<(Vec<Vec<f64>>, Vec<Vec<f64>>)> {
    if width == 0 || height == 0 || points.is_empty() {
        return Ok((
            vec![vec![f64::NEG_INFINITY; width]; height],
            vec![vec![f64::NEG_INFINITY; width]; height],
        ));
    }
    let ctx = gpu_raster_context().map_err(|e| anyhow::anyhow!(e))?;
    let (x_min, _y_min, _x_max, y_max) = bbox;

    let gpu_points: Vec<GpuLidarPoint> = points
        .iter()
        .map(|p| GpuLidarPoint {
            x: p.x as f32,
            y: p.y as f32,
            z: p.z as f32,
            classification: p.classification as u32,
        })
        .collect();

    let params = GpuRasterParams {
        x_min: x_min as f32,
        y_max: y_max as f32,
        resolution: resolution as f32,
        width: width as u32,
        height: height as u32,
        _pad: [0; 3],
    };

    let cell_count = width * height;
    let empty_bits = f32_to_sortable_u32(f32::NEG_INFINITY);
    let init_grid = vec![empty_bits; cell_count];

    let params_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_params"),
        size: std::mem::size_of::<GpuRasterParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

    let points_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_points"),
        size: (gpu_points.len() * std::mem::size_of::<GpuLidarPoint>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&points_buf, 0, bytemuck::cast_slice(&gpu_points));

    let grid_bytes = (cell_count * std::mem::size_of::<u32>()) as u64;
    let dsm_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_dsm"),
        size: grid_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let dtm_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_dtm"),
        size: grid_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&dsm_buf, 0, bytemuck::cast_slice(&init_grid));
    ctx.queue
        .write_buffer(&dtm_buf, 0, bytemuck::cast_slice(&init_grid));

    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lidar_staging"),
        size: grid_bytes * 2,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lidar_max_z_bg"),
        layout: &ctx.bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: points_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: dsm_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: dtm_buf.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lidar_max_z_enc"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("lidar_max_z_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&ctx.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let workgroups = (gpu_points.len() as u32).div_ceil(256);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&dsm_buf, 0, &staging, 0, grid_bytes);
    encoder.copy_buffer_to_buffer(&dtm_buf, 0, &staging, grid_bytes, grid_bytes);

    let submission_index = ctx.queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    ctx.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission_index),
            timeout: None,
        })
        .map_err(|e| anyhow::anyhow!("GPU poll failed: {e:?}"))?;
    receiver
        .recv()
        .context("GPU map channel closed")?
        .map_err(|e| anyhow::anyhow!("Failed to map GPU staging buffer: {e:?}"))?;

    let data = slice.get_mapped_range();
    let words: &[u32] = bytemuck::cast_slice(&data);
    let mut dsm = vec![vec![f64::NEG_INFINITY; width]; height];
    let mut dtm = vec![vec![f64::NEG_INFINITY; width]; height];
    for row in 0..height {
        for col in 0..width {
            let idx = row * width + col;
            let dsm_v = sortable_u32_to_f32(words[idx]);
            let dtm_v = sortable_u32_to_f32(words[cell_count + idx]);
            if dsm_v.is_finite() {
                dsm[row][col] = dsm_v as f64;
            }
            if dtm_v.is_finite() {
                dtm[row][col] = dtm_v as f64;
            }
        }
    }
    drop(data);
    staging.unmap();
    Ok((dsm, dtm))
}
