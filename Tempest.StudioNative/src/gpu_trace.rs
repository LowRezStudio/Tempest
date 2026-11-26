//! The GPU ray tracer of path-traced light bakes (`light.bake {pathTraced}`,
//! `tempest_assets::lightbake::trace`): the bake's tree (`Bvh::flat`) in
//! storage buffers, one compute invocation per ray walking it the way
//! `Bvh::walk` does (slabs, Möller–Trumbore, the nearer child first, a
//! 64-entry stack), rays sent and answers read back in batches. Any GPU wgpu
//! can run compute on will do; no ray tracing hardware is needed. A device of
//! its own, asking for the adapter's largest storage buffers (a big map's
//! triangles pass 128 MB).

use std::sync::Arc;

use tempest_assets::lightbake::bvh::{Bvh, Hit};
use tempest_assets::lightbake::trace::{Ray, Tracer, TracerFactory};
use wgpu::util::DeviceExt;

/// Rays per dispatch (32 bytes each in, 16 out).
const BATCH: usize = 1 << 20;
const GROUP: u32 = 64;

const SHADER: &str = r#"
struct Node { mn: vec3<f32>, start: u32, mx: vec3<f32>, count: u32 }
struct RayIn { o: vec3<f32>, tmax: f32, d: vec3<f32>, pad: f32 }
struct HitOut { t: f32, tri: u32, u: f32, v: f32 }
struct Params { count: u32, any: u32, pad0: u32, pad1: u32 }

@group(0) @binding(0) var<storage, read> nodes: array<Node>;
@group(0) @binding(1) var<storage, read> tris: array<f32>;
@group(0) @binding(2) var<storage, read> ids: array<u32>;
@group(0) @binding(3) var<storage, read> rays: array<RayIn>;
@group(0) @binding(4) var<storage, read_write> hits: array<HitOut>;
@group(0) @binding(5) var<uniform> params: Params;

const NONE: u32 = 0xffffffffu;

// The entry distance into a box, or -1 when the ray misses it before `limit`.
fn slab(mn: vec3<f32>, mx: vec3<f32>, o: vec3<f32>, inv: vec3<f32>, limit: f32) -> f32 {
    let a = (mn - o) * inv;
    let c = (mx - o) * inv;
    let lo = min(a, c);
    let hi = max(a, c);
    let t0 = max(max(max(lo.x, lo.y), lo.z), 0.0);
    let t1 = min(min(min(hi.x, hi.y), hi.z), limit);
    if (t0 > t1) { return -1.0; }
    return t0;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) { return; }
    let r = rays[i];
    var out = HitOut(0.0, NONE, 0.0, 0.0);
    if (arrayLength(&nodes) == 0u || arrayLength(&ids) == 0u) { hits[i] = out; return; }
    let o = r.o;
    let d = r.d;
    let inv = vec3<f32>(1.0) / d;
    var limit = r.tmax;
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
    loop {
        if (sp == 0u) { break; }
        sp = sp - 1u;
        let n = nodes[stack[sp]];
        if (slab(n.mn, n.mx, o, inv, limit) < 0.0) { continue; }
        if (n.count > 0u) {
            for (var k = n.start; k < n.start + n.count; k = k + 1u) {
                let b = k * 9u;
                let p0 = vec3<f32>(tris[b], tris[b + 1u], tris[b + 2u]);
                let p1 = vec3<f32>(tris[b + 3u], tris[b + 4u], tris[b + 5u]);
                let p2 = vec3<f32>(tris[b + 6u], tris[b + 7u], tris[b + 8u]);
                let e1 = p1 - p0;
                let e2 = p2 - p0;
                let pv = cross(d, e2);
                let det = dot(e1, pv);
                if (abs(det) < 1e-12) { continue; }
                let id = 1.0 / det;
                let s = o - p0;
                let u = dot(s, pv) * id;
                if (u < 0.0 || u > 1.0) { continue; }
                let q = cross(s, e1);
                let v = dot(d, q) * id;
                if (v < 0.0 || u + v > 1.0) { continue; }
                let t = dot(e2, q) * id;
                if (t > 0.0 && t < limit) {
                    out = HitOut(t, ids[k], u, v);
                    if (params.any != 0u) { hits[i] = out; return; }
                    limit = t;
                }
            }
        } else if (sp + 2u <= 64u) {
            let l = n.start;
            let rr = n.start + 1u;
            let dl = slab(nodes[l].mn, nodes[l].mx, o, inv, limit);
            let dr = slab(nodes[rr].mn, nodes[rr].mx, o, inv, limit);
            if (dl >= 0.0 && dr >= 0.0) {
                if (dl <= dr) { stack[sp] = rr; stack[sp + 1u] = l; } else { stack[sp] = l; stack[sp + 1u] = rr; }
                sp = sp + 2u;
            } else if (dl >= 0.0) {
                stack[sp] = l; sp = sp + 1u;
            } else if (dr >= 0.0) {
                stack[sp] = rr; sp = sp + 1u;
            }
        }
    }
    hits[i] = out;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RayIn {
    o: [f32; 3],
    tmax: f32,
    d: [f32; 3],
    pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct HitOut {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
}

struct Device {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    name: String,
}

impl Device {
    fn new() -> Result<Device, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(format!("{} is a software renderer", info.name));
        }
        let have = adapter.limits();
        let required_limits = wgpu::Limits { max_storage_buffer_binding_size: have.max_storage_buffer_binding_size, max_buffer_size: have.max_buffer_size, max_storage_buffers_per_shader_stage: have.max_storage_buffers_per_shader_stage.max(5), ..wgpu::Limits::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("light bake tracer"), required_limits, ..Default::default() })).map_err(|e| format!("no GPU device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("trace"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, true),
                storage(3, true),
                storage(4, false),
                wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("trace"), layout: Some(&pl), module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        Ok(Device { device, queue, layout, pipeline, name: format!("GPU ({})", info.name) })
    }
}

/// One scene's tree on the GPU.
pub struct GpuTracer {
    gpu: Arc<Device>,
    nodes: wgpu::Buffer,
    tris: wgpu::Buffer,
    ids: wgpu::Buffer,
    /// Serialises the batches of one tracer (the bake asks from many threads).
    lock: std::sync::Mutex<()>,
}

static DEVICE: std::sync::OnceLock<Result<Arc<Device>, String>> = std::sync::OnceLock::new();

impl GpuTracer {
    pub fn new(bvh: &Bvh) -> Result<GpuTracer, String> {
        let gpu = DEVICE.get_or_init(|| Device::new().map(Arc::new)).clone()?;
        let (nodes, tris, ids) = bvh.flat();
        let limit = gpu.device.limits().max_storage_buffer_binding_size;
        if (tris.len() * 4) as u64 > limit || (nodes.len() * 32) as u64 > limit {
            return Err(format!("the scene ({} triangles) is larger than the GPU's storage buffers ({} MB)", ids.len(), limit >> 20));
        }
        // Empty buffers cannot be bound: one zero element stands in.
        let buf = |bytes: &[u8]| gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: if bytes.is_empty() { &[0u8; 32] } else { bytes }, usage: wgpu::BufferUsages::STORAGE });
        Ok(GpuTracer { nodes: buf(bytemuck::cast_slice(&nodes)), tris: buf(bytemuck::cast_slice(&tris)), ids: buf(bytemuck::cast_slice(ids)), gpu, lock: std::sync::Mutex::new(()) })
    }

    fn run(&self, rays: &[Ray], any: bool) -> Vec<HitOut> {
        let _one = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::with_capacity(rays.len());
        let dev = &self.gpu.device;
        for chunk in rays.chunks(BATCH) {
            let input: Vec<RayIn> = chunk.iter().map(|r| RayIn { o: r.o, tmax: r.tmax, d: r.d, pad: 0.0 }).collect();
            let rays_buf = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&input), usage: wgpu::BufferUsages::STORAGE });
            let size = (chunk.len() * std::mem::size_of::<HitOut>()) as u64;
            let hits = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let readback = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let params = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&[chunk.len() as u32, any as u32, 0, 0]), usage: wgpu::BufferUsages::UNIFORM });
            let group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.gpu.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.nodes.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.tris.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.ids.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: rays_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: hits.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: params.as_entire_binding() },
                ],
            });
            let mut enc = dev.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.gpu.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups((chunk.len() as u32).div_ceil(GROUP), 1, 1);
            }
            enc.copy_buffer_to_buffer(&hits, 0, &readback, 0, size);
            self.gpu.queue.submit([enc.finish()]);
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = dev.poll(wgpu::PollType::wait_indefinitely());
            // A lost device fails the bake (the backend turns the panic into
            // its error) rather than lighting it with rays that hit nothing.
            let data = slice.get_mapped_range().unwrap_or_else(|e| panic!("the GPU stopped during the bake: {e:?}"));
            out.extend_from_slice(bytemuck::cast_slice(&data));
            drop(data);
            readback.unmap();
        }
        out
    }
}

impl Tracer for GpuTracer {
    fn name(&self) -> String {
        self.gpu.name.clone()
    }

    fn closest(&self, rays: &[Ray]) -> Vec<Option<Hit>> {
        self.run(rays, false).into_iter().map(|h| (h.tri != u32::MAX).then_some(Hit { t: h.t, tri: h.tri, u: h.u, v: h.v })).collect()
    }

    fn occluded(&self, rays: &[Ray]) -> Vec<bool> {
        self.run(rays, true).into_iter().map(|h| h.tri != u32::MAX).collect()
    }
}

/// What the backend makes a bake's tracer with.
pub fn factory() -> Box<TracerFactory> {
    Box::new(|bvh: &Bvh| GpuTracer::new(bvh).map(|t| Box::new(t) as Box<dyn Tracer>))
}
