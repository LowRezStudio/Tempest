//! The mesh preview: the backend's `mesh.preview` GLB (metres, Y up, base
//! colour textures resolved through the material chain) drawn with wgpu into
//! an off-screen texture, read back as BGRA and shown as a GPUI image. GPUI
//! does not lend its GPU device, so the viewer has its own; a frame is drawn
//! only when the camera or the size changes.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use gpui::RenderImage;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniforms {
    pub view_proj: [[f32; 4]; 4],
    pub light: [f32; 4],
    pub base: [f32; 4],
}

const SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, light: vec4<f32>, base: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut { @builtin(position) pos: vec4<f32>, @location(0) normal: vec3<f32>, @location(1) uv: vec2<f32> };

@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = u.view_proj * vec4<f32>(pos, 1.0);
    o.normal = normal;
    o.uv = uv;
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    // Two-sided: cooked meshes are often single-sided cards.
    let d = abs(dot(n, normalize(u.light.xyz)));
    let c = textureSample(tex, samp, i.uv).rgb * u.base.rgb;
    return vec4<f32>(c * (0.35 + 0.75 * d), 1.0);
}
"#;

/// Lines (the grid, the skeleton): the position and a colour in the normal's slot.
const LINE_SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, light: vec4<f32>, base: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec3<f32> };
@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) color: vec3<f32>, @location(2) uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = u.view_proj * vec4<f32>(pos, 1.0);
    o.color = color;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
"#;

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub(crate) const SAMPLES: u32 = 4;
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

struct Primitive {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    material: wgpu::BindGroup,
    /// Skinned on the CPU when the GLB has an animation.
    skin: Option<SkinnedPrim>,
}

/// A skinned primitive's bind-pose vertices and their joints.
struct SkinnedPrim {
    skin: usize,
    base: Vec<Vertex>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

struct Skin {
    joints: Vec<usize>,
    inverse_bind: Vec<Mat4>,
}

enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<glam::Quat>),
    Scale(Vec<Vec3>),
}

struct Channel {
    node: usize,
    times: Vec<f32>,
    values: Values,
}

/// The skeleton and the first animation of a GLB.
struct Animation {
    parents: Vec<Option<usize>>,
    rest: Vec<(Vec3, glam::Quat, Vec3)>,
    skins: Vec<Skin>,
    channels: Vec<Channel>,
    length: f32,
}

impl Animation {
    /// Every node's world matrix at time `t`.
    fn pose(&self, t: f32) -> Vec<Mat4> {
        let mut local = self.rest.clone();
        for c in &self.channels {
            if c.times.is_empty() {
                continue;
            }
            let k = c.times.partition_point(|&x| x <= t).saturating_sub(1).min(c.times.len() - 1);
            let k1 = (k + 1).min(c.times.len() - 1);
            let span = c.times[k1] - c.times[k];
            let f = if span > 0.0 { ((t - c.times[k]) / span).clamp(0.0, 1.0) } else { 0.0 };
            let l = &mut local[c.node];
            match &c.values {
                Values::Translation(v) => l.0 = v[k].lerp(v[k1], f),
                Values::Rotation(v) => l.1 = v[k].slerp(v[k1], f),
                Values::Scale(v) => l.2 = v[k].lerp(v[k1], f),
            }
        }
        let mut world: Vec<Option<Mat4>> = vec![None; local.len()];
        fn get(i: usize, parents: &[Option<usize>], local: &[(Vec3, glam::Quat, Vec3)], world: &mut Vec<Option<Mat4>>) -> Mat4 {
            if let Some(m) = world[i] {
                return m;
            }
            let (t, r, s) = local[i];
            let m = Mat4::from_scale_rotation_translation(s, r, t);
            let m = match parents[i] {
                Some(p) => get(p, parents, local, world) * m,
                None => m,
            };
            world[i] = Some(m);
            m
        }
        (0..local.len()).map(|i| get(i, &self.parents, &local, &mut world)).collect()
    }
}

/// Orbit camera around the mesh's bounds.
#[derive(Clone, Copy)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

/// The GPU device and the pipeline, made once for every viewer.
pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    /// Lines instead of triangles (`None` when the GPU cannot draw them).
    wireframe: Option<wgpu::RenderPipeline>,
    pub white: wgpu::BindGroup,
    /// Line lists drawn over everything (no depth test).
    pub lines: wgpu::RenderPipeline,
    pub globals_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
}

static GPU: std::sync::OnceLock<Result<Arc<Gpu>, String>> = std::sync::OnceLock::new();

pub(crate) fn gpu() -> Result<Arc<Gpu>, String> {
    GPU.get_or_init(|| Gpu::new().map(Arc::new)).clone()
}

pub struct MeshView {
    gpu: Arc<Gpu>,
    uniforms: wgpu::Buffer,
    globals: wgpu::BindGroup,
    primitives: Vec<Primitive>,
    pub center: Vec3,
    pub radius: f32,
    pub camera: Camera,
    pub triangles: usize,
    /// Draw with the textures (else a flat grey), as lines.
    pub textured: bool,
    pub wireframe: bool,
    /// The background, linear RGB (the window theme's).
    pub clear: [f64; 3],
    anim: Option<Animation>,
    /// The skeleton: every node's parent, its rest transform, the first skin's joints and the pose shown.
    bones: Option<Bones>,
    /// The lowest point: where the grid lies.
    floor: f32,
    pub show_grid: bool,
    pub show_skeleton: bool,
}

struct Bones {
    parents: Vec<Option<usize>>,
    joints: Vec<usize>,
    world: Vec<Mat4>,
}

impl MeshView {
    /// The animation's length in seconds (0: none).
    pub fn length(&self) -> f32 {
        self.anim.as_ref().map(|a| a.length).unwrap_or(0.0)
    }

    /// Pose the skinned primitives at `t` seconds of the animation.
    pub fn set_time(&mut self, t: f32) {
        let Some(anim) = &self.anim else { return };
        let world = anim.pose(t);
        if let Some(b) = &mut self.bones {
            b.world = world.clone();
        }
        let mats: Vec<Vec<Mat4>> = anim.skins.iter().map(|s| s.joints.iter().zip(&s.inverse_bind).map(|(&j, ib)| world[j] * *ib).collect()).collect();
        for p in &self.primitives {
            let Some(sk) = &p.skin else { continue };
            let m = &mats[sk.skin];
            let out: Vec<Vertex> = sk
                .base
                .iter()
                .enumerate()
                .map(|(k, v)| {
                    let (j, w) = (sk.joints[k], sk.weights[k]);
                    let mut skin = Mat4::ZERO;
                    for n in 0..4 {
                        if w[n] > 0.0 {
                            skin += *m.get(j[n] as usize).unwrap_or(&Mat4::IDENTITY) * w[n];
                        }
                    }
                    if skin == Mat4::ZERO {
                        skin = Mat4::IDENTITY;
                    }
                    let pos = skin.transform_point3(Vec3::from(v.pos));
                    let normal = skin.transform_vector3(Vec3::from(v.normal)).normalize_or_zero();
                    Vertex { pos: pos.into(), normal: normal.into(), uv: v.uv }
                })
                .collect();
            self.gpu.queue.write_buffer(&p.vertices, 0, bytemuck::cast_slice(&out));
        }
    }

    /// Vertices of the skeleton's lines (they come after the grid's).
    fn skeleton_lines(&self) -> usize {
        match (&self.bones, self.show_skeleton) {
            (Some(b), true) => b.joints.iter().filter(|&&j| b.parents[j].is_some_and(|p| b.joints.contains(&p))).count() * 2,
            _ => 0,
        }
    }

    pub fn has_skeleton(&self) -> bool {
        self.bones.as_ref().is_some_and(|b| b.joints.len() > 1)
    }

    /// The grid and skeleton lines to draw now.
    fn lines(&self) -> Vec<Vertex> {
        let mut out = if self.show_grid { grid(self.center, self.radius, self.floor) } else { Vec::new() };
        let mut line = |a: Vec3, b: Vec3, c: [f32; 3]| {
            out.push(Vertex { pos: a.into(), normal: c, uv: [0.0; 2] });
            out.push(Vertex { pos: b.into(), normal: c, uv: [0.0; 2] });
        };
        if self.show_skeleton {
            if let Some(b) = &self.bones {
                for &j in &b.joints {
                    if let Some(p) = b.parents[j].filter(|p| b.joints.contains(p)) {
                        line(b.world[p].w_axis.truncate(), b.world[j].w_axis.truncate(), [0.98, 0.7, 0.53]);
                    }
                }
            }
        }
        out
    }

    pub fn can_wireframe(&self) -> bool {
        self.gpu.wireframe.is_some()
    }
}

impl Gpu {
    fn new() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).map_err(|e| format!("no GPU adapter: {e}"))?;
        let lines = adapter.features().contains(wgpu::Features::POLYGON_MODE_LINE);
        let desc = wgpu::DeviceDescriptor { required_features: if lines { wgpu::Features::POLYGON_MODE_LINE } else { wgpu::Features::empty() }, ..Default::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&desc)).map_err(|e| format!("no GPU device: {e}"))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mesh"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)], immediate_size: 0 });
        let make = |mode: wgpu::PolygonMode| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), targets: &[Some(FORMAT.into())], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { polygon_mode: mode, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let pipeline = make(wgpu::PolygonMode::Fill);
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lines"), source: wgpu::ShaderSource::Wgsl(LINE_SHADER.into()) });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&globals_layout)], immediate_size: 0 });
        let lines_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lines"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &line_shader, entry_point: Some("fs"), targets: &[Some(FORMAT.into())], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::Always), stencil: Default::default(), bias: Default::default() }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let wireframe = lines.then(|| make(wgpu::PolygonMode::Line));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let white_tex = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[200, 200, 200, 255],
        );
        let white_view = white_tex.create_view(&Default::default());
        let white = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&white_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }] });
        Ok(Gpu { device, queue, pipeline, wireframe, white, lines: lines_pipe, globals_layout, material_layout, sampler })
    }
}

impl MeshView {
    /// Upload a GLB. Errors are messages for the status line.
    pub fn new(glb: &[u8]) -> Result<MeshView, String> {
        let gpu = gpu()?;
        let (device, queue, sampler, material_layout) = (&gpu.device, &gpu.queue, &gpu.sampler, &gpu.material_layout);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &gpu.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }] });

        // The GLB: every mesh node's primitives in world space, the base colour textures.
        let gltf = gltf::Gltf::from_slice(glb).map_err(|e| format!("GLB: {e}"))?;
        let blob = gltf.blob.as_deref().unwrap_or_default();
        let buffer_data = |b: gltf::Buffer| -> Option<&[u8]> { matches!(b.source(), gltf::buffer::Source::Bin).then_some(blob) };
        let texture = |rgba: &[u8], w: u32, h: u32| -> wgpu::TextureView {
            let t = device.create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                wgpu::util::TextureDataOrder::LayerMajor,
                rgba,
            );
            t.create_view(&Default::default())
        };
        let white = texture(&[255, 255, 255, 255], 1, 1);
        // Decoded images by glTF image index.
        let mut images: Vec<Option<wgpu::TextureView>> = Vec::new();
        for img in gltf.images() {
            let view = match img.source() {
                gltf::image::Source::View { view, .. } => {
                    let start = view.offset();
                    blob.get(start..start + view.length()).and_then(|png| image::load_from_memory(png).ok()).map(|i| {
                        let i = i.to_rgba8();
                        texture(&i, i.width(), i.height())
                    })
                }
                _ => None,
            };
            images.push(view);
        }
        let material_group = |view: &wgpu::TextureView| device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) }] });

        // The skeleton and animation, when the GLB has them (animation previews).
        let anim = gltf.animations().next().map(|a| {
            let n = gltf.nodes().count();
            let mut parents = vec![None; n];
            for node in gltf.nodes() {
                for c in node.children() {
                    parents[c.index()] = Some(node.index());
                }
            }
            let rest = gltf.nodes().map(|node| {
                let (t, r, sc) = node.transform().decomposed();
                (Vec3::from(t), glam::Quat::from_array(r), Vec3::from(sc))
            }).collect();
            let skins = gltf.skins().map(|sk| {
                let reader = sk.reader(buffer_data);
                let joints: Vec<usize> = sk.joints().map(|j| j.index()).collect();
                let inverse_bind = reader.read_inverse_bind_matrices().map(|m| m.map(|x| Mat4::from_cols_array_2d(&x)).collect()).unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
                Skin { joints, inverse_bind }
            }).collect();
            let mut length = 0f32;
            let channels = a.channels().filter_map(|c| {
                let reader = c.reader(buffer_data);
                let times: Vec<f32> = reader.read_inputs()?.collect();
                length = length.max(times.last().copied().unwrap_or(0.0));
                use gltf::animation::util::ReadOutputs;
                let values = match reader.read_outputs()? {
                    ReadOutputs::Translations(v) => Values::Translation(v.map(Vec3::from).collect()),
                    ReadOutputs::Rotations(v) => Values::Rotation(v.into_f32().map(glam::Quat::from_array).collect()),
                    ReadOutputs::Scales(v) => Values::Scale(v.map(Vec3::from).collect()),
                    _ => return None,
                };
                Some(Channel { node: c.target().node().index(), times, values })
            }).collect();
            Animation { parents, rest, skins, channels, length }
        });
        let mut primitives = Vec::new();
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut triangles = 0;
        let scene = gltf.default_scene().or_else(|| gltf.scenes().next()).ok_or("GLB without a scene")?;
        let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
        while let Some((node, parent)) = stack.pop() {
            let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            stack.extend(node.children().map(|c| (c, world)));
            let Some(mesh) = node.mesh() else { continue };
            // A skinned mesh is posed by its joints, not by its node.
            let skinned = anim.is_some() && node.skin().is_some();
            let world = if skinned { Mat4::IDENTITY } else { world };
            let normal_m = world.inverse().transpose();
            for prim in mesh.primitives() {
                let reader = prim.reader(buffer_data);
                let Some(pos) = reader.read_positions() else { continue };
                let pos: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from(p))).collect();
                let normals: Vec<Vec3> = reader.read_normals().map(|n| n.map(|n| normal_m.transform_vector3(Vec3::from(n)).normalize_or_zero()).collect()).unwrap_or_else(|| vec![Vec3::Y; pos.len()]);
                let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0; 2]; pos.len()]);
                let indices: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
                for p in &pos {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                let vertices: Vec<Vertex> = (0..pos.len()).map(|k| Vertex { pos: pos[k].into(), normal: normals.get(k).copied().unwrap_or(Vec3::Y).into(), uv: uvs.get(k).copied().unwrap_or_default() }).collect();
                let base = prim.material().pbr_metallic_roughness().base_color_texture().and_then(|t| images.get(t.texture().source().index()).and_then(|v| v.as_ref()));
                triangles += indices.len() / 3;
                let skin = match (skinned, reader.read_joints(0), reader.read_weights(0)) {
                    (true, Some(j), Some(w)) => Some(SkinnedPrim { skin: node.skin().map(|s| s.index()).unwrap_or(0), base: vertices.clone(), joints: j.into_u16().collect(), weights: w.into_f32().collect() }),
                    _ => None,
                };
                primitives.push(Primitive {
                    skin,
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX }),
                    count: indices.len() as u32,
                    material: material_group(base.unwrap_or(&white)),
                });
            }
        }
        if primitives.is_empty() {
            return Err("the mesh has no geometry".into());
        }
        let center = (lo + hi) * 0.5;
        let radius = ((hi - lo).length() * 0.5).max(0.01);
        // The first skin's joints at rest, for the skeleton overlay.
        let bones = gltf.skins().next().map(|sk| {
            let n = gltf.nodes().count();
            let mut parents = vec![None; n];
            for node in gltf.nodes() {
                for c in node.children() {
                    parents[c.index()] = Some(node.index());
                }
            }
            let local: Vec<Mat4> = gltf.nodes().map(|node| Mat4::from_cols_array_2d(&node.transform().matrix())).collect();
            let mut world = vec![Mat4::IDENTITY; n];
            let mut done = vec![false; n];
            fn get(i: usize, parents: &[Option<usize>], local: &[Mat4], world: &mut [Mat4], done: &mut [bool]) -> Mat4 {
                if !done[i] {
                    let m = match parents[i] {
                        Some(p) => get(p, parents, local, world, done) * local[i],
                        None => local[i],
                    };
                    world[i] = m;
                    done[i] = true;
                }
                world[i]
            }
            for i in 0..n {
                get(i, &parents, &local, &mut world, &mut done);
            }
            Bones { parents, joints: sk.joints().map(|j| j.index()).collect(), world }
        });
        let mut v = MeshView { gpu: gpu.clone(), uniforms, globals, primitives, center, radius, camera: Camera { yaw: 0.6, pitch: 0.25, distance: radius * 2.6 }, triangles, textured: true, wireframe: false, clear: [0.016, 0.016, 0.027], anim, bones, floor: lo.y, show_grid: false, show_skeleton: false };
        if v.anim.is_some() {
            v.set_time(0.0);
        }
        Ok(v)
    }

    /// A UV sphere with `texture` as its base colour, for material previews.
    pub fn sphere(texture: Option<&image::RgbaImage>) -> Result<MeshView, String> {
        let gpu = gpu()?;
        let device = &gpu.device;
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Uniforms>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &gpu.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }] });
        let (rings, segments) = (48u32, 96u32);
        let mut vertices = Vec::new();
        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let phi = v * std::f32::consts::PI;
            for k in 0..=segments {
                let u = k as f32 / segments as f32;
                let theta = u * std::f32::consts::TAU;
                let n = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
                // The texture tiles twice around, like the Electron preview.
                vertices.push(Vertex { pos: n.into(), normal: n.into(), uv: [u * 2.0, v] });
            }
        }
        let mut indices = Vec::new();
        for r in 0..rings {
            for k in 0..segments {
                let a = r * (segments + 1) + k;
                let b = a + segments + 1;
                indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        let view = texture.map(|t| {
            let tex = device.create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: t.width(), height: t.height(), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                wgpu::util::TextureDataOrder::LayerMajor,
                t.as_raw(),
            );
            tex.create_view(&Default::default())
        });
        let material = match &view {
            Some(v) => device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &gpu.material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&gpu.sampler) }] }),
            None => gpu.white.clone(),
        };
        let primitives = vec![Primitive {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX }),
            count: indices.len() as u32,
            material,
            skin: None,
        }];
        let triangles = indices.len() / 3;
        Ok(MeshView { gpu: gpu.clone(), uniforms, globals, primitives, center: Vec3::ZERO, radius: 1.0, camera: Camera { yaw: 0.6, pitch: 0.25, distance: 2.9 }, triangles, textured: true, wireframe: false, clear: [0.016, 0.016, 0.027], anim: None, bones: None, floor: -1.0, show_grid: false, show_skeleton: false })
    }

    /// Draw a `width`×`height` frame (device pixels).
    pub fn render(&self, width: u32, height: u32) -> Arc<RenderImage> {
        let (w, h) = (width.max(16), height.max(16));
        let (view_proj, eye) = view_proj(self.center, self.radius, self.camera, w, h);
        let u = Uniforms { view_proj: view_proj.to_cols_array_2d(), light: [eye.x - self.center.x + self.radius, eye.y - self.center.y + self.radius * 2.0, eye.z - self.center.z, 0.0], base: [1.0; 4] };
        self.gpu.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        // The grid first: the mesh covers it; the skeleton is drawn over everything below.
        let lines = self.lines();
        let line_buf = (!lines.is_empty()).then(|| self.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&lines), usage: wgpu::BufferUsages::VERTEX }));
        let grid_count = if self.show_grid { lines.len() - self.skeleton_lines() } else { 0 } as u32;
        let mesh_pipeline = match (&self.gpu.wireframe, self.wireframe) {
            (Some(lines), true) => lines,
            _ => &self.gpu.pipeline,
        };
        draw_frame(&self.gpu, w, h, self.clear, |pass| {
            pass.set_bind_group(0, &self.globals, &[]);
            if let (Some(buf), true) = (&line_buf, grid_count > 0) {
                pass.set_pipeline(&self.gpu.lines);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..grid_count, 0..1);
            }
            pass.set_pipeline(mesh_pipeline);
            for p in &self.primitives {
                pass.set_bind_group(1, if self.textured { &p.material } else { &self.gpu.white }, &[]);
                pass.set_vertex_buffer(0, p.vertices.slice(..));
                pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..p.count, 0, 0..1);
            }
            if let Some(buf) = &line_buf {
                if (lines.len() as u32) > grid_count {
                    pass.set_pipeline(&self.gpu.lines);
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(grid_count..lines.len() as u32, 0..1);
                }
            }
        })
    }

    /// Every primitive: vertices, indices, index count and its material (for instanced drawing).
    pub(crate) fn primitives(&self) -> impl Iterator<Item = (&wgpu::Buffer, &wgpu::Buffer, u32, &wgpu::BindGroup)> {
        self.primitives.iter().map(|p| (&p.vertices, &p.indices, p.count, &p.material))
    }
}

/// The orbit camera's view-projection for a `w`×`h` frame, and where the eye is.
pub(crate) fn view_proj(center: Vec3, radius: f32, c: Camera, w: u32, h: u32) -> (Mat4, Vec3) {
    let eye = center + c.distance * Vec3::new(c.yaw.sin() * c.pitch.cos(), c.pitch.sin(), c.yaw.cos() * c.pitch.cos());
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let proj = Mat4::perspective_rh(45f32.to_radians(), w as f32 / h as f32, (c.distance - radius * 2.0).max(radius * 0.01), c.distance + radius * 2.0);
    (proj * view, eye)
}

/// A square grid around `center` at height `floor`, lines at a round step.
pub(crate) fn grid(center: Vec3, radius: f32, floor: f32) -> Vec<Vertex> {
    let mut out = Vec::new();
    let mut line = |a: Vec3, b: Vec3, c: [f32; 3]| {
        out.push(Vertex { pos: a.into(), normal: c, uv: [0.0; 2] });
        out.push(Vertex { pos: b.into(), normal: c, uv: [0.0; 2] });
    };
    let half = radius * 1.5;
    let raw = half / 5.0;
    let base = 10f32.powf(raw.log10().floor());
    let step = base * if raw / base >= 5.0 { 5.0 } else if raw / base >= 2.0 { 2.0 } else { 1.0 };
    let n = (half / step).ceil() as i32;
    let (cx, cz, y) = ((center.x / step).round() * step, (center.z / step).round() * step, floor);
    for k in -n..=n {
        let o = k as f32 * step;
        let c = if k == 0 { [0.45, 0.47, 0.6] } else { [0.22, 0.23, 0.3] };
        line(Vec3::new(cx + o, y, cz - n as f32 * step), Vec3::new(cx + o, y, cz + n as f32 * step), c);
        line(Vec3::new(cx - n as f32 * step, y, cz + o), Vec3::new(cx + n as f32 * step, y, cz + o), c);
    }
    out
}

/// Draw one `w`×`h` frame with `draw` (multisampled, depth cleared to 1, the
/// background `clear`), resolve it and read it back as a GPUI image.
pub(crate) fn draw_frame(gpu: &Gpu, w: u32, h: u32, clear: [f64; 3], draw: impl FnOnce(&mut wgpu::RenderPass<'_>)) -> Arc<RenderImage> {
    draw_frame_depth(gpu, w, h, clear, 1.0, draw)
}

/// `draw_frame` with the depth cleared to `depth` (0 for a reversed depth buffer).
pub(crate) fn draw_frame_depth(gpu: &Gpu, w: u32, h: u32, clear: [f64; 3], depth_clear: f32, draw: impl FnOnce(&mut wgpu::RenderPass<'_>)) -> Arc<RenderImage> {
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    // Drawn multisampled, resolved into `target`.
    let msaa = gpu.device.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: SAMPLES, dimension: wgpu::TextureDimension::D2, format: FORMAT, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: SAMPLES, dimension: wgpu::TextureDimension::D2, format: DEPTH, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let (tv, mv, dv) = (target.create_view(&Default::default()), msaa.create_view(&Default::default()), depth.create_view(&Default::default()));
    // Rows of the copy are padded to 256 bytes.
    let row = (w * 4).div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &mv, depth_slice: None, resolve_target: Some(&tv), ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: clear[0], g: clear[1], b: clear[2], a: 1.0 }), store: wgpu::StoreOp::Discard } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(depth_clear), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        draw(&mut pass);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        size,
    );
    gpu.queue.submit([enc.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let Ok(data) = slice.get_mapped_range() else { return Arc::new(RenderImage::new(Vec::<image::Frame>::new())) };
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let start = y * row as usize;
        bgra.extend_from_slice(&data[start..start + w as usize * 4]);
    }
    drop(data);
    readback.unmap();
    let frame = image::Frame::new(image::RgbaImage::from_raw(w, h, bgra).expect("frame size"));
    Arc::new(RenderImage::new(smallvec_one(frame)))
}

fn smallvec_one(frame: image::Frame) -> Vec<image::Frame> {
    vec![frame]
}
