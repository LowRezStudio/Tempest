//! The particle preview's scene: every emitter's simulation (`particles.rs`)
//! drawn with the mesh viewer's device (`mesh_view.rs`). Sprite emitters are
//! instanced camera-facing quads (or locked to an axis) with the material's
//! base texture, blend mode and sub-UV sheet; mesh emitters draw their mesh
//! (`mesh.preview`) once per particle, lit, tinted by the particle colour.
//! The simulation runs in Unreal units, Z up; the view is in metres, Y up
//! like the mesh previews: (x, y, z) → (x, z, y) / 100.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use glam::{EulerRot, Mat4, Quat, Vec3};
use gpui::RenderImage;
use wgpu::util::DeviceExt;

use crate::mesh_view::{self, Camera, Gpu, MeshView, Uniforms, Vertex, DEPTH, FORMAT, SAMPLES};
use crate::particles::{orbit_offset, EmitterModel, EmitterSim};

const SCALE: f32 = 0.01;

fn to_view(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1]) * SCALE
}

const SPRITE_SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, right: vec4<f32>, up: vec4<f32>, sub: vec4<f32>, mode: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32> };

@vertex fn vs(@builtin(vertex_index) vi: u32, @location(0) pos: vec3<f32>, @location(1) size: vec2<f32>, @location(2) rot_sub: vec2<f32>, @location(3) color: vec4<f32>) -> VOut {
    var corners = array<vec2<f32>, 6>(vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(0.5, 0.5), vec2(-0.5, -0.5), vec2(0.5, 0.5), vec2(-0.5, 0.5));
    let c = corners[vi];
    let cs = cos(rot_sub.x);
    let sn = sin(rot_sub.x);
    let r = vec2(c.x * cs - c.y * sn, c.x * sn + c.y * cs) * size;
    var world: vec3<f32>;
    let align = i32(u.sub.z + 0.5);
    if (align == 1) {
        world = pos + vec3(r.x, 0.0, -r.y);
    } else if (align == 2) {
        world = pos + vec3(0.0, r.y, r.x);
    } else if (align == 3) {
        world = pos + vec3(r.x, r.y, 0.0);
    } else {
        world = pos + u.right.xyz * r.x + u.up.xyz * r.y;
    }
    var o: VOut;
    o.pos = u.view_proj * vec4<f32>(world, 1.0);
    // Sub-UV: frame `idx` of a sheet of sub.x × sub.y, left to right, top to bottom.
    let idx = floor(rot_sub.y + 0.5);
    let col = idx - floor(idx / u.sub.x) * u.sub.x;
    let row = floor(idx / u.sub.x);
    o.uv = (vec2(c.x + 0.5, 0.5 - c.y) + vec2(col, row)) / u.sub.xy;
    o.color = color;
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    var t = vec4<f32>(1.0);
    if (u.sub.w > 0.5) {
        t = textureSample(tex, samp, i.uv);
    }
    let c = t * i.color;
    // Masked materials cut at their clip value; nothing draws nearly transparent pixels.
    if (c.a < max(u.mode.x, 0.002)) {
        discard;
    }
    return c;
}
"#;

const MESH_SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, light: vec4<f32>, base: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) normal: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32> };

@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
              @location(3) m0: vec4<f32>, @location(4) m1: vec4<f32>, @location(5) m2: vec4<f32>, @location(6) m3: vec4<f32>, @location(7) color: vec4<f32>) -> VOut {
    let m = mat4x4<f32>(m0, m1, m2, m3);
    var o: VOut;
    o.pos = u.view_proj * m * vec4<f32>(pos, 1.0);
    o.normal = (m * vec4<f32>(normal, 0.0)).xyz;
    o.uv = uv;
    o.color = color;
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let d = abs(dot(normalize(i.normal), normalize(u.light.xyz)));
    let c = textureSample(tex, samp, i.uv).rgb * i.color.rgb;
    return vec4<f32>(c * (0.35 + 0.75 * d), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteUniforms {
    view_proj: [[f32; 4]; 4],
    right: [f32; 4],
    up: [f32; 4],
    /// Sub-images across and down, the axis lock (0 faces the camera), has a texture.
    sub: [f32; 4],
    /// The alpha below which pixels are cut.
    mode: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteInstance {
    pos: [f32; 3],
    size: [f32; 2],
    rot_sub: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshInstance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

/// How a material blends, from its `BlendMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    Translucent,
    Additive,
    Modulate,
    Masked,
    Opaque,
}

impl Blend {
    pub fn of(mode: &str) -> Blend {
        let m = mode.to_ascii_lowercase();
        if m.contains("additive") {
            Blend::Additive
        } else if m.contains("modulate") {
            Blend::Modulate
        } else if m.contains("masked") {
            Blend::Masked
        } else if m.contains("opaque") {
            Blend::Opaque
        } else {
            Blend::Translucent
        }
    }

    const ALL: [Blend; 5] = [Blend::Translucent, Blend::Additive, Blend::Modulate, Blend::Masked, Blend::Opaque];
}

/// The particle pipelines, made once on the shared device.
struct Pipes {
    sprite: HashMap<Blend, wgpu::RenderPipeline>,
    sprite_layout: wgpu::BindGroupLayout,
    mesh: wgpu::RenderPipeline,
}

static PIPES: OnceLock<Pipes> = OnceLock::new();

fn pipes(gpu: &Gpu) -> &'static Pipes {
    PIPES.get_or_init(|| {
        let device = &gpu.device;
        let sprite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&sprite_layout), Some(&gpu.material_layout)], immediate_size: 0 });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sprites"), source: wgpu::ShaderSource::Wgsl(SPRITE_SHADER.into()) });
        // The frame stays opaque: blending never changes the target's alpha.
        let keep_alpha = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let color = |src, dst| wgpu::BlendState { color: wgpu::BlendComponent { src_factor: src, dst_factor: dst, operation: wgpu::BlendOperation::Add }, alpha: keep_alpha };
        let sprite = Blend::ALL
            .into_iter()
            .map(|b| {
                use wgpu::BlendFactor as F;
                let blend = match b {
                    Blend::Translucent => Some(color(F::SrcAlpha, F::OneMinusSrcAlpha)),
                    Blend::Additive => Some(color(F::SrcAlpha, F::One)),
                    Blend::Modulate => Some(color(F::Zero, F::Src)),
                    Blend::Masked | Blend::Opaque => None,
                };
                let writes = matches!(b, Blend::Masked | Blend::Opaque);
                let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("sprites"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<SpriteInstance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4] })],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(writes), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
                    multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                    multiview_mask: None,
                    cache: None,
                });
                (b, pipe)
            })
            .collect();
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mesh particles"), source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()) });
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&gpu.globals_layout), Some(&gpu.material_layout)], immediate_size: 0 });
        let mesh = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh particles"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs"),
                buffers: &[
                    Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] }),
                    Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<MeshInstance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4] }),
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &mesh_shader, entry_point: Some("fs"), targets: &[Some(FORMAT.into())], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Pipes { sprite, sprite_layout, mesh }
    })
}

/// A sprite material: how it blends and its base texture (a PNG).
pub struct Material {
    pub blend: Blend,
    pub png: Option<Vec<u8>>,
}

enum EmitterGpu {
    Sprite { blend: Blend, has_map: bool, material: wgpu::BindGroup, uniforms: wgpu::Buffer, globals: wgpu::BindGroup, instances: wgpu::Buffer, cap: usize },
    /// `material`: the required module's texture, when the emitter overrides the mesh's materials.
    Mesh { mesh: Arc<MeshView>, material: Option<wgpu::BindGroup>, instances: wgpu::Buffer, cap: usize },
}

pub struct Runtime {
    pub sim: EmitterSim,
    pub visible: bool,
    gpu: Option<EmitterGpu>,
}

pub struct ParticleScene {
    gpu: Arc<Gpu>,
    pub runtimes: Vec<Runtime>,
    pub camera: Camera,
    pub center: Vec3,
    pub radius: f32,
    pub clear: [f64; 3],
    pub show_grid: bool,
    floor: f32,
    /// Uniforms of the mesh pipeline and the grid.
    uniforms: wgpu::Buffer,
    globals: wgpu::BindGroup,
}

impl ParticleScene {
    /// Upload every emitter: `materials` by material path, `meshes` (GLBs of `mesh.preview`) by mesh path.
    pub fn new(models: Vec<EmitterModel>, materials: &HashMap<String, Material>, meshes: &HashMap<String, Vec<u8>>) -> Result<ParticleScene, String> {
        let gpu = mesh_view::gpu()?;
        let pipes = pipes(&gpu);
        let device = &gpu.device;
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &gpu.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }] });
        // Textures by material, meshes by path: emitters share them.
        let mut textures: HashMap<String, Option<wgpu::BindGroup>> = HashMap::new();
        let mut mesh_views: HashMap<String, Option<Arc<MeshView>>> = HashMap::new();
        let mut runtimes = Vec::new();
        for (k, model) in models.into_iter().enumerate() {
            let cap = model.cap();
            let material = model.material.as_ref().and_then(|m| materials.get(m));
            let key = model.material.clone().unwrap_or_default();
            let bind = textures
                    .entry(key)
                    .or_insert_with(|| {
                        let img = material.and_then(|m| m.png.as_ref()).and_then(|png| image::load_from_memory(png).ok())?.to_rgba8();
                        let tex = device.create_texture_with_data(
                            &gpu.queue,
                            &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                            wgpu::util::TextureDataOrder::LayerMajor,
                            img.as_raw(),
                        );
                        let view = tex.create_view(&Default::default());
                        Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &gpu.material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&gpu.sampler) }] }))
                    })
                    .clone();
            let view = if let Some(path) = &model.mesh {
                let mesh = mesh_views.entry(path.clone()).or_insert_with(|| meshes.get(path).and_then(|glb| MeshView::new(glb).ok()).map(Arc::new)).clone();
                let material = bind.filter(|_| model.override_material);
                mesh.map(|mesh| EmitterGpu::Mesh { mesh, material, instances: device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (cap * std::mem::size_of::<MeshInstance>()) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), cap })
            } else {
                let su = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<SpriteUniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                let sg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &pipes.sprite_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: su.as_entire_binding() }] });
                Some(EmitterGpu::Sprite {
                    blend: material.map(|m| m.blend).unwrap_or(Blend::Translucent),
                    has_map: bind.is_some(),
                    material: bind.unwrap_or_else(|| gpu.white.clone()),
                    uniforms: su,
                    globals: sg,
                    instances: device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (cap * std::mem::size_of::<SpriteInstance>()) as u64, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }),
                    cap,
                })
            };
            runtimes.push(Runtime { sim: EmitterSim::new(model, k as u32 + 1), visible: true, gpu: view });
        }
        Ok(ParticleScene { gpu: gpu.clone(), runtimes, camera: Camera { yaw: 0.8, pitch: 0.35, distance: 2.6 }, center: Vec3::new(0.0, 0.5, 0.0), radius: 1.0, clear: [0.016, 0.016, 0.027], show_grid: true, floor: 0.0, uniforms, globals })
    }

    /// Advance every shown emitter by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        for r in self.runtimes.iter_mut().filter(|r| r.visible) {
            r.sim.step(dt);
        }
    }

    /// Start every emitter over (the same seeds: the same particles).
    pub fn restart(&mut self) {
        for (k, r) in self.runtimes.iter_mut().enumerate() {
            r.sim.reset(k as u32 + 1);
        }
    }

    /// One-shot effects that have played out.
    pub fn finished(&self) -> bool {
        !self.runtimes.is_empty() && self.runtimes.iter().all(|r| r.sim.finished && r.sim.particles.is_empty())
    }

    pub fn count(&self) -> usize {
        self.runtimes.iter().filter(|r| r.visible).map(|r| r.sim.particles.len()).sum()
    }

    /// Frame the particles alive now (a unit box above the origin when there are none).
    pub fn fit(&mut self) {
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for r in self.runtimes.iter().filter(|r| r.visible) {
            for p in &r.sim.particles {
                let v = to_view(p.pos);
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if !lo.is_finite() {
            (lo, hi) = (Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5));
        }
        let size = hi - lo;
        self.center = (lo + hi) * 0.5;
        self.radius = size.max_element().max(0.5);
        self.floor = lo.y.min(0.0);
        self.camera = Camera { yaw: 0.8, pitch: 0.35, distance: self.radius * 2.6 };
    }

    pub fn render(&self, width: u32, height: u32) -> Arc<RenderImage> {
        let (w, h) = (width.max(16), height.max(16));
        let gpu = &self.gpu;
        let pipes = pipes(gpu);
        let (view_proj, eye) = mesh_view::view_proj(self.center, self.radius, self.camera, w, h);
        let u = Uniforms { view_proj: view_proj.to_cols_array_2d(), light: [eye.x - self.center.x + self.radius, eye.y - self.center.y + self.radius * 2.0, eye.z - self.center.z, 0.0], base: [1.0; 4] };
        gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        // The camera's right and up, for sprites facing it.
        let forward = (self.center - eye).normalize_or(Vec3::NEG_Z);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        // Instances of this frame.
        let mut draws: Vec<(usize, u32)> = Vec::new();
        for (k, r) in self.runtimes.iter().enumerate() {
            let Some(g) = &r.gpu else { continue };
            let ps: &[_] = if r.visible { &r.sim.particles } else { &[] };
            match g {
                EmitterGpu::Sprite { instances, cap, uniforms, has_map, blend, .. } => {
                    let n = ps.len().min(*cap);
                    let data: Vec<SpriteInstance> = ps[..n]
                        .iter()
                        .map(|p| {
                            let o = orbit_offset(p);
                            SpriteInstance { pos: to_view([p.pos[0] + o[0], p.pos[1] + o[1], p.pos[2] + o[2]]).into(), size: [p.size[0] * SCALE, p.size[1] * SCALE], rot_sub: [p.rot, p.sub], color: p.color }
                        })
                        .collect();
                    if n > 0 {
                        gpu.queue.write_buffer(instances, 0, bytemuck::cast_slice(&data));
                    }
                    let m = &r.sim.model;
                    let align = if m.axis_lock.contains("_z") { 1.0 } else if m.axis_lock.contains("_x") { 2.0 } else if m.axis_lock.contains("_y") { 3.0 } else { 0.0 };
                    let su = SpriteUniforms {
                        view_proj: view_proj.to_cols_array_2d(),
                        right: right.extend(0.0).into(),
                        up: up.extend(0.0).into(),
                        sub: [m.sub_h, m.sub_v, align, if *has_map { 1.0 } else { 0.0 }],
                        mode: [if *blend == Blend::Masked { 0.33 } else { 0.0 }, 0.0, 0.0, 0.0],
                    };
                    gpu.queue.write_buffer(uniforms, 0, bytemuck::bytes_of(&su));
                    draws.push((k, n as u32));
                }
                EmitterGpu::Mesh { instances, cap, .. } => {
                    let n = ps.len().min(*cap);
                    let data: Vec<MeshInstance> = ps[..n]
                        .iter()
                        .map(|p| {
                            let o = orbit_offset(p);
                            let t = to_view([p.pos[0] + o[0], p.pos[1] + o[1], p.pos[2] + o[2]]);
                            let q = Quat::from_euler(EulerRot::XYZ, p.mesh_rot[0], p.mesh_rot[2], -p.mesh_rot[1]);
                            let s = Vec3::new(p.size[0].max(0.001), p.size[2].max(0.001), p.size[1].max(0.001));
                            MeshInstance { model: Mat4::from_scale_rotation_translation(s, q, t).to_cols_array_2d(), color: p.color }
                        })
                        .collect();
                    if n > 0 {
                        gpu.queue.write_buffer(instances, 0, bytemuck::cast_slice(&data));
                    }
                    draws.push((k, n as u32));
                }
            }
        }
        let lines = if self.show_grid { mesh_view::grid(self.center, self.radius, self.floor) } else { Vec::new() };
        let line_buf = (!lines.is_empty()).then(|| gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&lines), usage: wgpu::BufferUsages::VERTEX }));
        mesh_view::draw_frame(gpu, w, h, self.clear, |pass| {
            pass.set_bind_group(0, &self.globals, &[]);
            if let Some(buf) = &line_buf {
                pass.set_pipeline(&gpu.lines);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..lines.len() as u32, 0..1);
            }
            // Meshes and cut-out sprites first: they write depth, the blended sprites test against it.
            let order = |k: usize| match &self.runtimes[k].gpu {
                Some(EmitterGpu::Mesh { .. }) => 0,
                Some(EmitterGpu::Sprite { blend: Blend::Masked | Blend::Opaque, .. }) => 1,
                _ => 2,
            };
            let mut sorted = draws.clone();
            sorted.sort_by_key(|(k, _)| order(*k));
            for (k, n) in sorted {
                if n == 0 {
                    continue;
                }
                match &self.runtimes[k].gpu {
                    Some(EmitterGpu::Sprite { blend, material, globals, instances, .. }) => {
                        pass.set_pipeline(&pipes.sprite[blend]);
                        pass.set_bind_group(0, globals, &[]);
                        pass.set_bind_group(1, material, &[]);
                        pass.set_vertex_buffer(0, instances.slice(..));
                        pass.draw(0..6, 0..n);
                    }
                    Some(EmitterGpu::Mesh { mesh, material: over, instances, .. }) => {
                        pass.set_pipeline(&pipes.mesh);
                        pass.set_bind_group(0, &self.globals, &[]);
                        pass.set_vertex_buffer(1, instances.slice(..));
                        for (vertices, indices, count, material) in mesh.primitives() {
                            pass.set_bind_group(1, over.as_ref().unwrap_or(material), &[]);
                            pass.set_vertex_buffer(0, vertices.slice(..));
                            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..count, 0, 0..n);
                        }
                    }
                    None => {}
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::{Dist, EmitterModel};

    fn constant(prop: &str, v: &[f32]) -> Dist {
        Dist { prop: prop.into(), vector: v.len() == 3, op: 0, num_elements: 1, chunk: v.len(), comps: v.len(), time_scale: 0.0, start_time: 0.0, entries: vec![v.to_vec()] }
    }

    /// Every pipeline builds, and a red additive emitter shows on the frame.
    #[test]
    fn renders_sprites() {
        let model = EmitterModel {
            name: "test".into(),
            enabled: true,
            duration: 1.0,
            sub_h: 1.0,
            sub_v: 1.0,
            sub_method: "none".into(),
            material: Some("M".into()),
            rate: Some(constant("Rate", &[200.0])),
            lifetime: Some(constant("Lifetime", &[2.0])),
            size: Some(constant("StartSize", &[40.0, 40.0, 40.0])),
            color: Some(constant("StartColor", &[1.0, 0.0, 0.0])),
            velocity: Some(constant("StartVelocity", &[0.0, 0.0, 30.0])),
            peak: 64,
            ..Default::default()
        };
        let materials = HashMap::from([("M".to_string(), Material { blend: Blend::Additive, png: None })]);
        // CI machines without a GPU adapter skip the drawing.
        let mut scene = match ParticleScene::new(vec![model], &materials, &HashMap::new()) {
            Ok(s) => s,
            Err(e) if e.starts_with("no GPU") => return eprintln!("skipped: {e}"),
            Err(e) => panic!("{e}"),
        };
        for _ in 0..30 {
            scene.step(1.0 / 30.0);
        }
        assert!(scene.count() > 150);
        scene.fit();
        scene.show_grid = false;
        let image = scene.render(64, 64);
        let frame = image.as_bytes(0).expect("a frame");
        // BGRA: the sprites add red over the dark background.
        let red = frame.as_chunks::<4>().0.iter().filter(|p| p[2] > 200 && p[1] < 80 && p[0] < 80).count();
        assert!(red > 20, "{red} red pixels");
    }
}
