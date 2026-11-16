//! Previews drawn by the game's own shaders: the WGSL of
//! `material.gameShader` (a material's Direct3D 9 base pass pair,
//! translated) as a [`GamePass`], fed like the local vertex factory feeds a
//! static mesh and lit by one directional light and a sky light that follow
//! the camera. The material preview draws one on a sphere
//! ([`GameMaterial`]); the mesh preview one per section (`mesh_view`).
//!
//! The world is the game's: Z up, left-handed, Unreal units. Constants are
//! filled by the names the shaders' constant tables give them: the
//! material's (`UniformPixelVector_N`, `UniformPixelScalars_N`: its uniform
//! expressions, evaluated each frame with the instance's parameters and the
//! clock) and the renderer's (view, light, sky). Matrices go in as the
//! engine stores them: register `k` holds the matrix's `k`th row in the
//! row-vector convention, which is glam's column `k`. Renderer textures
//! (light attenuation, shadows, cube maps) are white: lit, unshadowed.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use gpui::RenderImage;
use serde_json::Value as J;
use tempest_assets::shadercache::{UniformExpr, UniformInputs};
use wgpu::util::DeviceExt;

use crate::mesh_view::{draw_frame, gpu, Camera, Gpu, DEPTH, FORMAT, SAMPLES};

/// Which game shaders previews ask for: shader model 5 (Direct3D 11, what
/// the game runs by default) unless shader model 3 is chosen.
static SM3: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_sm3(on: bool) {
    SM3.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The `platform` of a `material.gameShader` request.
pub fn platform() -> &'static str {
    if SM3.load(std::sync::atomic::Ordering::Relaxed) { "sm3" } else { "sm5" }
}

/// The sphere's radius, in Unreal units (a player is about 180 tall).
pub const RADIUS: f32 = 64.0;

/// A vertex as the local vertex factory takes it, in the game's space: the
/// binormal is `cross(normal, tangent) * sign`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameVertex {
    pub pos: Vec3,
    pub normal: Vec3,
    pub tangent: Vec3,
    pub sign: f32,
    pub uv: [f32; 2],
}

/// The key light of a level's sun, and of a preview's world asset.
const LEVEL_KEY: [f32; 3] = [1.6, 1.55, 1.45];
/// The key light of a preview's character: the lobby lights champions with
/// brightness 3 to 9 (LobbyMapV4_P, LobbyMapV3_Char), and the character master
/// materials scale their own direct diffuse by a quarter, so the level's key
/// leaves skin and cloth too dark (Evie's skin01 looks tanned).
const LOBBY_KEY: [f32; 3] = [6.4, 6.2, 5.8];

/// What the renderer's constants follow in a frame: the camera (game space).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub target: Vec3,
    pub near: f32,
    /// The direction to the light (a level's sun); `None`: over the viewer's shoulder.
    pub light: Option<Vec3>,
}

/// How a pass is drawn: the target's depth convention and culling.
#[derive(Clone, Copy, Debug)]
pub struct PassOptions {
    /// Depth cleared to 0 and compared greater-or-equal (the map view's reversed depth).
    pub reversed_depth: bool,
    /// Back faces culled unless the material is two-sided (placements may be mirrored: off in maps).
    pub cull: bool,
}

impl Default for PassOptions {
    fn default() -> PassOptions {
        PassOptions { reversed_depth: false, cull: true }
    }
}

/// A placement's vertex constants: `LocalToWorld` (4 rows), `WorldToLocal`
/// (3 rows) and the determinant's sign, worked out once per placement.
pub type Placement = [[f32; 4]; 8];

/// The [`Placement`] of `local_to_world` (the game's space).
pub fn placement(local_to_world: Mat4) -> Placement {
    let l = local_to_world.to_cols_array_2d();
    let w = local_to_world.inverse().to_cols_array_2d();
    [l[0], l[1], l[2], l[3], w[0], w[1], w[2], [local_to_world.determinant().signum(), 0.0, 0.0, 0.0]]
}

/// One stage's float constants: where each is (by name: first float and
/// how many, SM5 packing scalars into a register's components) and the
/// values uploaded.
struct Constants {
    names: Vec<(String, usize, usize)>,
    data: Vec<[f32; 4]>,
}

impl Constants {
    fn new(list: &J, floats: usize) -> Constants {
        let names = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let register = c["register"].as_u64().unwrap_or(0) as usize;
                let count = c["count"].as_u64().unwrap_or(1) as usize;
                let offset = c["offset"].as_u64().map(|o| o as usize).unwrap_or(register * 16);
                let bytes = c["bytes"].as_u64().map(|b| b as usize).unwrap_or(count * 16);
                (c["name"].as_str().unwrap_or_default().to_ascii_lowercase(), offset / 4, bytes / 4)
            })
            .collect();
        Constants { names, data: vec![[0.0; 4]; floats.max(1)] }
    }

    /// Write `rows` to the constant called `name` (no-op when the stage does not read it).
    fn set(&mut self, name: &str, rows: &[[f32; 4]]) {
        let mut data = std::mem::take(&mut self.data);
        self.set_in(&mut data, name, rows);
        self.data = data;
    }

    /// [`Constants::set`] into another copy of the registers.
    fn set_in(&self, data: &mut [[f32; 4]], name: &str, rows: &[[f32; 4]]) {
        let Some(&(_, first, floats)) = self.names.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name)) else { return };
        let flat: &mut [f32] = bytemuck::cast_slice_mut(data);
        let values = rows.iter().flatten().take(floats);
        for (k, v) in values.enumerate() {
            if let Some(slot) = flat.get_mut(first + k) {
                *slot = *v;
            }
        }
    }

    fn reads(&self, name: &str) -> bool {
        self.names.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(name))
    }

    fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.data)
    }
}

/// The instance's parameters (lower case names) and the clock: what uniform expressions read.
#[derive(Clone, Default)]
pub struct Params {
    pub scalars: HashMap<String, f32>,
    pub vectors: HashMap<String, [f32; 4]>,
    pub time: f32,
}

impl Params {
    /// The `scalars` and `vectors` of a `material.preview` / `material.gameShader` answer.
    pub fn from_json(j: &J) -> Params {
        let scalars = j["scalars"].as_array().into_iter().flatten().filter_map(|s| Some((s["name"].as_str()?.to_ascii_lowercase(), s["value"].as_f64()? as f32))).collect();
        let vectors = j["vectors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let v: Vec<f32> = s["value"].as_array()?.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect();
                Some((s["name"].as_str()?.to_ascii_lowercase(), [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0), v.get(3).copied().unwrap_or(1.0)]))
            })
            .collect();
        let mut p = Params { scalars, vectors, time: 0.0 };
        p.runtime();
        p
    }

    /// What the game drives at run time rather than the material: the lobby's
    /// selection glow (`TgSkeletalMeshActor_Lobby` fades `SelectionGlow_Fade` in
    /// for 0.6 s when a champion is picked; at rest it is 0, while the
    /// material's default, 1, is the glow at full strength).
    pub fn runtime(&mut self) {
        self.scalars.insert("selectionglow_fade".into(), 0.0);
    }
}

impl UniformInputs for Params {
    fn scalar(&self, name: &str) -> Option<f32> {
        self.scalars.get(&name.to_ascii_lowercase()).copied()
    }
    fn vector(&self, name: &str) -> Option<[f32; 4]> {
        self.vectors.get(&name.to_ascii_lowercase()).copied()
    }
    fn time(&self) -> (f32, f32) {
        (self.time, self.time)
    }
}

/// How the material blends (`BlendMode`): what the pipeline's blend state follows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Blend {
    Opaque,
    Translucent,
    Additive,
    Modulate,
}

impl Blend {
    fn of(mode: &str) -> Blend {
        match mode.to_ascii_lowercase().trim_start_matches("blend_") {
            "translucent" | "alphacomposite" | "dithered_translucent" => Blend::Translucent,
            "additive" => Blend::Additive,
            "modulate" | "modulateandadd" => Blend::Modulate,
            _ => Blend::Opaque,
        }
    }

    /// The colour blends; alpha keeps the cleared 1, so the frame stays opaque
    /// (the shader's alpha is the scene depth for opaque materials).
    fn state(self) -> wgpu::BlendState {
        use wgpu::{BlendComponent as C, BlendFactor as F, BlendOperation as O};
        let keep = C { src_factor: F::Zero, dst_factor: F::One, operation: O::Add };
        let color = match self {
            Blend::Opaque => C::REPLACE,
            Blend::Translucent => C { src_factor: F::SrcAlpha, dst_factor: F::OneMinusSrcAlpha, operation: O::Add },
            Blend::Additive => C { src_factor: F::One, dst_factor: F::One, operation: O::Add },
            Blend::Modulate => C { src_factor: F::Dst, dst_factor: F::Zero, operation: O::Add },
        };
        wgpu::BlendState { color, alpha: keep }
    }
}

/// What a vertex input of the local vertex factory holds for a sphere point:
/// the position, the tangent basis packed (Direct3D 9 binds it as `UBYTE4`,
/// 0..255; Direct3D 11 as UNORM, 0..1, `unorm`; either shader unpacks it),
/// texture coordinates, white vertex colour.
fn vertex_input(semantic: &str, v: &GameVertex, unorm: bool) -> [f32; 4] {
    let scale = if unorm { 1.0 } else { 255.0 };
    let pack = |v: Vec3, w: f32| [(v.x * 0.5 + 0.5) * scale, (v.y * 0.5 + 0.5) * scale, (v.z * 0.5 + 0.5) * scale, w * scale];
    let usage = semantic.trim_end_matches(|c: char| c.is_ascii_digit());
    match usage {
        "POSITION" => [v.pos.x, v.pos.y, v.pos.z, 1.0],
        "TANGENT" => pack(v.tangent, 1.0),
        // The normal's w is the basis determinant's sign.
        "NORMAL" => pack(v.normal, if v.sign < 0.0 { 0.0 } else { 1.0 }),
        "TEXCOORD" => [v.uv[0], v.uv[1], 0.0, 1.0],
        "COLOR" => [1.0; 4],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

/// The sphere: its vertices and triangles wound clockwise seen from outside (Direct3D's front faces).
fn sphere() -> (Vec<GameVertex>, Vec<u32>) {
    let (rings, segments) = (48u32, 96u32);
    let mut data = Vec::new();
    let mut points = Vec::new();
    for r in 0..=rings {
        let v = r as f32 / rings as f32;
        let phi = v * std::f32::consts::PI;
        for k in 0..=segments {
            let u = k as f32 / segments as f32;
            let theta = u * std::f32::consts::TAU;
            let n = Vec3::new(phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos());
            // dP/du (around) and dP/dv (down): the tangent and the binormal the mesh importer would compute.
            let t = Vec3::new(-theta.sin(), theta.cos(), 0.0);
            let b = Vec3::new(phi.cos() * theta.cos(), phi.cos() * theta.sin(), -phi.sin());
            let sign = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
            // The texture tiles twice around, like the base colour preview.
            points.push(n);
            data.push(GameVertex { pos: n * RADIUS, normal: n, tangent: t, sign, uv: [u * 2.0, v] });
        }
    }
    let mut indices = Vec::new();
    for r in 0..rings {
        for k in 0..segments {
            let a = r * (segments + 1) + k;
            let b = a + segments + 1;
            for tri in [[a, b, a + 1], [a + 1, b, b + 1]] {
                let (p0, p1, p2) = (points[tri[0] as usize], points[tri[1] as usize], points[tri[2] as usize]);
                let n = (p1 - p0).cross(p2 - p0);
                // Degenerate at the poles: any order.
                if n.dot(p0 + p1 + p2) >= 0.0 {
                    indices.extend_from_slice(&tri);
                } else {
                    indices.extend_from_slice(&[tri[0], tri[2], tri[1]]);
                }
            }
        }
    }
    (data, indices)
}

/// RGBA8 mips of `img`, down to 1×1.
fn mips(img: image::RgbaImage) -> Vec<image::RgbaImage> {
    let mut out = vec![img];
    loop {
        let last = out.last().expect("one mip");
        let (w, h) = (last.width(), last.height());
        if w <= 1 && h <= 1 {
            break;
        }
        let next = image::imageops::resize(last, (w / 2).max(1), (h / 2).max(1), image::imageops::FilterType::Triangle);
        out.push(next);
    }
    out
}

/// A material's game shaders on the GPU: the pipeline, its constants and textures.
pub struct GamePass {
    gpu: Arc<Gpu>,
    pipeline: wgpu::RenderPipeline,
    /// The vertex inputs, in `@location` order, and whether the tangent basis is UNORM (SM5).
    inputs: Vec<String>,
    unorm: bool,
    vs_buf: wgpu::Buffer,
    ps_buf: wgpu::Buffer,
    globals: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    groups: Vec<wgpu::BindGroup>,
    /// The placements' vertex constants (`draws` of the WGSL, one block per instance):
    /// their buffer, one block's bytes, how many blocks it holds,
    /// and where each constant sits in a block (name, slot, rows).
    draw_buf: wgpu::Buffer,
    stride: usize,
    capacity: usize,
    draw_slots: Vec<(String, usize, usize)>,
    vsc: std::sync::Mutex<Constants>,
    psc: std::sync::Mutex<Constants>,
    pixel_vectors: Vec<UniformExpr>,
    pixel_scalars: Vec<UniformExpr>,
    vertex_vectors: Vec<UniformExpr>,
    vertex_scalars: Vec<UniformExpr>,
    pub params: Params,
    /// Blended (drawn after the opaque ones, without writing depth).
    pub translucent: bool,
    /// Whether a uniform expression reads the clock: the preview then redraws continuously.
    pub animated: bool,
    /// Compiled for skinned meshes (a champion's or a weapon's): previews light it like the lobby.
    pub character: bool,
    /// The shader pair, for the viewport's caption.
    pub pair: String,
    /// Samplers whose texture could not be had (shown under the preview).
    pub warnings: Vec<String>,
}

impl GamePass {
    /// Build the pipeline from a `material.gameShader` answer; `bin` reads its PNG payloads.
    pub fn new(j: &J, bin: &dyn Fn(&J) -> Option<Vec<u8>>) -> Result<GamePass, String> {
        GamePass::with_options(j, bin, PassOptions::default())
    }

    pub fn with_options(j: &J, bin: &dyn Fn(&J) -> Option<Vec<u8>>, options: PassOptions) -> Result<GamePass, String> {
        let gpu = gpu()?;
        let device = &gpu.device;
        let wgsl = j["wgsl"].as_str().ok_or("no shader in the answer")?;
        let inputs: Vec<String> = j["vertexInputs"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect();
        let (vfloats, pfloats) = (j["vertexFloats"].as_u64().unwrap_or(1) as usize, j["pixelFloats"].as_u64().unwrap_or(1) as usize);
        let vsc = Constants::new(&j["vertexConstants"], vfloats);
        let psc = Constants::new(&j["pixelConstants"], pfloats);
        let exprs = |k: &str| -> Vec<UniformExpr> { j[k].as_array().into_iter().flatten().map(|e| UniformExpr::from_json(e).unwrap_or(UniformExpr::Constant { value: [0.0; 4], value_type: 4 })).collect() };
        let (pixel_vectors, pixel_scalars, vertex_vectors, vertex_scalars) = (exprs("pixelVectors"), exprs("pixelScalars"), exprs("vertexVectors"), exprs("vertexScalars"));
        let animated = [&pixel_vectors, &pixel_scalars, &vertex_vectors, &vertex_scalars].iter().any(|l| l.iter().any(UniformExpr::uses_time));
        let blend = Blend::of(j["blendMode"].as_str().unwrap_or_default());
        let two_sided = j["twoSided"].as_bool().unwrap_or(false);

        // Validation errors are caught here rather than reported to the device's handler (which panics).
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("game material"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let draw_floats = j["drawFloats"].as_u64().unwrap_or(1).max(1) as usize;
        let draw_slots: Vec<(String, usize, usize)> = j["drawConstants"].as_array().into_iter().flatten().map(|c| (c["name"].as_str().unwrap_or_default().to_ascii_lowercase(), c["slot"].as_u64().unwrap_or(0) as usize, c["count"].as_u64().unwrap_or(1) as usize)).collect();
        // One block per placement, at the WGSL array's stride (`draws`, read by instance).
        let stride = draw_floats * 16;
        let buffer = |binding: u32, ty: wgpu::BufferBindingType, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let uniform = wgpu::BufferBindingType::Uniform;
        let draws = wgpu::BufferBindingType::Storage { read_only: true };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[buffer(0, uniform, wgpu::ShaderStages::VERTEX_FRAGMENT), buffer(1, uniform, wgpu::ShaderStages::VERTEX_FRAGMENT), buffer(2, draws, wgpu::ShaderStages::VERTEX)] });
        let capacity = 1;
        let vs_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: vsc.bytes(), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let ps_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: psc.bytes(), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let draw_buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (stride * capacity) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer(&draw_buf, 0, &placement_block(&draw_slots, &placement(Mat4::IDENTITY), stride));
        let globals = globals_group(device, &globals_layout, &vs_buf, &ps_buf, &draw_buf);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let mut warnings = Vec::new();
        // Group 1: the pixel shader's samplers; group 2: the vertex shader's.
        let mut layouts = Vec::new();
        let mut groups = Vec::new();
        for (key, stage) in [("samplers", wgpu::ShaderStages::FRAGMENT), ("vertexSamplers", wgpu::ShaderStages::VERTEX)] {
            let list = j[key].as_array().cloned().unwrap_or_default();
            if list.is_empty() && key == "vertexSamplers" {
                break;
            }
            let mut entries = Vec::new();
            let mut views = Vec::new();
            for (k, s) in list.iter().enumerate() {
                let dim = match s["kind"].as_str() {
                    Some("cube") => wgpu::TextureViewDimension::Cube,
                    Some("volume") => wgpu::TextureViewDimension::D3,
                    _ => wgpu::TextureViewDimension::D2,
                };
                let filterable = stage == wgpu::ShaderStages::FRAGMENT;
                entries.push(wgpu::BindGroupLayoutEntry { binding: 2 * k as u32, visibility: stage, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: dim, multisampled: false }, count: None });
                entries.push(wgpu::BindGroupLayoutEntry { binding: 2 * k as u32 + 1, visibility: stage, ty: wgpu::BindingType::Sampler(if filterable { wgpu::SamplerBindingType::Filtering } else { wgpu::SamplerBindingType::NonFiltering }), count: None });
                let decode = |v: &J| bin(v).and_then(|png| image::load_from_memory(&png).ok()).map(|i| i.to_rgba8());
                // A 2D texture's image, or a cube map's six faces (all of one size).
                let faces: Vec<image::RgbaImage> = match dim {
                    wgpu::TextureViewDimension::D2 => decode(&s["png"]).into_iter().collect(),
                    wgpu::TextureViewDimension::Cube => {
                        let f: Vec<image::RgbaImage> = s["faces"].as_array().into_iter().flatten().filter_map(decode).collect();
                        if f.len() == 6 && f.iter().all(|i| i.dimensions() == f[0].dimensions()) { f } else { Vec::new() }
                    }
                    _ => Vec::new(),
                };
                if s["source"] == "texture" && faces.is_empty() || s["source"] == "missing" {
                    warnings.push(format!("{}: {}", s["name"].as_str().unwrap_or_default(), s["error"].as_str().unwrap_or("not decoded")));
                }
                views.push(texture_view(&gpu, faces, s["srgb"].as_bool().unwrap_or(true), dim, filterable));
            }
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries });
            let point = device.create_sampler(&wgpu::SamplerDescriptor::default());
            let mut bind = Vec::new();
            for (k, v) in views.iter().enumerate() {
                bind.push(wgpu::BindGroupEntry { binding: 2 * k as u32, resource: wgpu::BindingResource::TextureView(v) });
                bind.push(wgpu::BindGroupEntry { binding: 2 * k as u32 + 1, resource: wgpu::BindingResource::Sampler(if stage == wgpu::ShaderStages::FRAGMENT { &sampler } else { &point }) });
            }
            groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &bind }));
            layouts.push(layout);
        }
        let mut all_layouts = vec![Some(&globals_layout)];
        all_layouts.extend(layouts.iter().map(Some));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &all_layouts, immediate_size: 0 });
        let attributes: Vec<wgpu::VertexAttribute> = (0..inputs.len()).map(|k| wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16 * k as u64, shader_location: k as u32 }).collect();
        let translucent = blend != Blend::Opaque;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("game material"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 16 * inputs.len().max(1) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_main"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: Some(blend.state()), write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, cull_mode: (options.cull && !two_sided).then_some(wgpu::Face::Back), ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(!translucent),
                depth_compare: Some(if options.reversed_depth { wgpu::CompareFunction::GreaterEqual } else { wgpu::CompareFunction::Less }),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the game shader does not build: {e}"));
        }
        let pair = format!("{} + {}", j["vertexShader"].as_str().unwrap_or_default(), j["pixelShader"].as_str().unwrap_or_default());
        Ok(GamePass {
            gpu: gpu.clone(),
            pipeline,
            inputs,
            unorm: j["platform"] == "sm5",
            character: j["otherFactories"].as_array().is_some_and(|f| f.iter().any(|f| f.as_str().is_some_and(|f| f.eq_ignore_ascii_case("FGPUSkinVertexFactory")))),
            vs_buf,
            ps_buf,
            globals,
            globals_layout,
            groups,
            draw_buf,
            stride,
            capacity,
            draw_slots,
            vsc: std::sync::Mutex::new(vsc),
            psc: std::sync::Mutex::new(psc),
            pixel_vectors,
            pixel_scalars,
            vertex_vectors,
            vertex_scalars,
            params: Params::from_json(j),
            translucent,
            animated,
            pair,
            warnings,
        })
    }

    /// The vertices as this pass's inputs take them: interleaved `vec4<f32>`s.
    pub fn vertex_data(&self, vertices: &[GameVertex]) -> Vec<[f32; 4]> {
        let mut out = Vec::with_capacity(vertices.len() * self.inputs.len().max(1));
        for v in vertices {
            if self.inputs.is_empty() {
                out.push([0.0; 4]);
            }
            for s in &self.inputs {
                out.push(vertex_input(s, v, self.unorm));
            }
        }
        out
    }

    /// A vertex buffer of `vertices` for this pass (rewritten with [`GamePass::vertex_data`] when they move).
    pub fn vertex_buffer(&self, vertices: &[GameVertex]) -> wgpu::Buffer {
        self.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&self.vertex_data(vertices)), usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST })
    }

    /// Fill and upload the constants for `frame`.
    pub fn prepare(&self, frame: &Frame) {
        self.scene_constants(frame);
        self.material_constants();
        self.gpu.queue.write_buffer(&self.vs_buf, 0, self.vsc.lock().unwrap_or_else(|e| e.into_inner()).bytes());
        self.gpu.queue.write_buffer(&self.ps_buf, 0, self.psc.lock().unwrap_or_else(|e| e.into_inner()).bytes());
    }

    /// The placements [`GamePass::draw_bound`] draws instances of (the game's space):
    /// written once, until the scene changes.
    pub fn set_placements(&mut self, placements: &[Placement]) {
        let n = placements.len().max(1);
        if n > self.capacity {
            self.capacity = n.next_power_of_two();
            self.draw_buf = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (self.stride * self.capacity) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.globals = globals_group(&self.gpu.device, &self.globals_layout, &self.vs_buf, &self.ps_buf, &self.draw_buf);
        }
        let mut bytes = Vec::with_capacity(self.stride * n);
        for p in placements {
            bytes.extend(placement_block(&self.draw_slots, p, self.stride));
        }
        self.gpu.queue.write_buffer(&self.draw_buf, 0, &bytes);
    }

    /// Set the pipeline, constants and textures once for a run of [`GamePass::draw_bound`]s.
    pub fn bind(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        for (g, group) in self.groups.iter().enumerate() {
            pass.set_bind_group(1 + g as u32, group, &[]);
        }
    }

    /// Draw placements `first..first + n` (one instance each), after [`GamePass::bind`].
    pub fn draw_bound(&self, pass: &mut wgpu::RenderPass<'_>, first: u32, n: u32, vertices: &wgpu::Buffer, indices: &wgpu::Buffer, count: u32) {
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..count, 0, first..first + n);
    }

    /// Draw `count` indices of `indices` over `vertices` (a [`GamePass::vertex_buffer`]) where [`GamePass::prepare`] left the camera.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, vertices: &wgpu::Buffer, indices: &wgpu::Buffer, count: u32) {
        self.bind(pass);
        self.draw_bound(pass, 0, 1, vertices, indices, count);
    }

    /// The camera, the light and the sky: the renderer's constants.
    fn scene_constants(&self, frame: &Frame) {
        let (eye, near) = (frame.eye, frame.near);
        let rows = |m: Mat4| m.to_cols_array_2d();
        // The light comes from above, over the viewer's left shoulder.
        let forward = (frame.target - eye).normalize_or_zero();
        let right = Vec3::Z.cross(forward).normalize_or_zero();
        let light = frame.light.unwrap_or_else(|| (-forward * 0.8 + Vec3::Z * 0.9 - right * 0.6).normalize());
        let mut v = self.vsc.lock().unwrap_or_else(|e| e.into_inner());
        v.set("ViewProjectionMatrix", &rows(frame.view_proj));
        v.set("LocalToWorld", &rows(Mat4::IDENTITY));
        v.set("WorldToLocal", &rows(Mat4::IDENTITY));
        v.set("CameraPositionVS", &[[eye.x, eye.y, eye.z, 1.0]]);
        v.set("CameraWorldPosition", &[[eye.x, eye.y, eye.z, 1.0]]);
        // A directional light's "position" is its direction, with no radius.
        v.set("LightPositionAndInvRadius", &[[light.x, light.y, light.z, 0.0]]);
        v.set("LightDirection", &[[light.x, light.y, light.z, 0.0]]);
        v.set("LocalToWorldRotDeterminantFlip", &[[1.0, 0.0, 0.0, 0.0]]);
        // No fog: the layered height fog (translucent base passes) starts beyond everything, which leaves
        // its transmittance at 1 (zeros would make it 0: black).
        v.set("bUseExponentialHeightFog", &[[0.0; 4]]);
        v.set("SharedFogParameter1", &[[f32::MAX; 4]]);
        let mut p = self.psc.lock().unwrap_or_else(|e| e.into_inner());
        p.set("ViewProjectionMatrix", &rows(frame.view_proj));
        let key = if frame.light.is_none() && self.character { LOBBY_KEY } else { LEVEL_KEY };
        p.set("LightColorAndFalloffExponent", &[[key[0], key[1], key[2], 0.0]]);
        p.set("UpperSkyColor", &[[0.28, 0.3, 0.36, 0.0]]);
        p.set("LowerSkyColor", &[[0.09, 0.08, 0.07, 0.0]]);
        p.set("AmbientColorAndSkyFactor", &[[0.02, 0.02, 0.025, 1.0]]);
        // Spherical-harmonic ambient (SH light passes): only the constant term, whose R, G and B
        // the shaders read from the first register and scale by sqrt(π)/2.
        p.set("WorldIncidentLighting", &[[0.12 / 0.886, 0.12 / 0.886, 0.13 / 0.886, 0.0]]);
        // Diffuse and specular as the material computes them (the editor's viewmodes override these).
        p.set("DiffuseOverrideParameter", &[[0.0, 0.0, 0.0, 1.0]]);
        p.set("SpecularOverrideParameter", &[[0.0, 0.0, 0.0, 1.0]]);
        p.set("MinZ_MaxZRatio", &[[near, 1.0, 0.0, 0.0]]);
        p.set("ScreenPositionScaleBias", &[[0.5, -0.5, 0.5, 0.5]]);
        let t = self.params.time;
        p.set("GTimeVariablesPS", &[[t, t, 0.0, 0.0]]);
    }

    /// The material's constants: its uniform expressions now.
    fn material_constants(&self) {
        let fill = |c: &mut Constants, vectors: &[UniformExpr], scalars: &[UniformExpr], vector: &str, scalar: &str| {
            for (k, e) in vectors.iter().enumerate() {
                let name = format!("{vector}{k}");
                if c.reads(&name) {
                    c.set(&name, &[e.eval(&self.params)]);
                }
            }
            for (k, chunk) in scalars.chunks(4).enumerate() {
                let name = format!("{scalar}{k}");
                if c.reads(&name) {
                    let mut v = [0.0; 4];
                    for (i, e) in chunk.iter().enumerate() {
                        v[i] = e.eval(&self.params)[0];
                    }
                    c.set(&name, &[v]);
                }
            }
        };
        fill(&mut self.psc.lock().unwrap_or_else(|e| e.into_inner()), &self.pixel_vectors, &self.pixel_scalars, "UniformPixelVector_", "UniformPixelScalars_");
        fill(&mut self.vsc.lock().unwrap_or_else(|e| e.into_inner()), &self.vertex_vectors, &self.vertex_scalars, "UniformVertexVector_", "UniformVertexScalars_");
    }

}

/// The material preview: a [`GamePass`] on a sphere, orbited in the game's space.
pub struct GameMaterial {
    pub pass: GamePass,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    pub camera: Camera,
    pub clear: [f64; 3],
}

impl GameMaterial {
    pub fn new(pass: GamePass) -> GameMaterial {
        let (data, indices) = sphere();
        let vertices = pass.vertex_buffer(&data);
        let index_buf = pass.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        GameMaterial { pass, vertices, indices: index_buf, count: indices.len() as u32, camera: Camera { yaw: 0.6, pitch: 0.25, distance: RADIUS * 2.9 }, clear: [0.016, 0.016, 0.027] }
    }

    pub fn render(&self, width: u32, height: u32) -> Arc<RenderImage> {
        let (w, h) = (width.max(16), height.max(16));
        let c = self.camera;
        let eye = c.distance * Vec3::new(c.pitch.cos() * c.yaw.cos(), c.pitch.cos() * c.yaw.sin(), c.pitch.sin());
        let near = (c.distance - RADIUS * 2.0).max(RADIUS * 0.01);
        let proj = Mat4::perspective_lh(45f32.to_radians(), w as f32 / h as f32, near, c.distance + RADIUS * 2.0);
        let frame = Frame { view_proj: proj * Mat4::look_at_lh(eye, Vec3::ZERO, Vec3::Z), eye, target: Vec3::ZERO, near, light: None };
        self.pass.prepare(&frame);
        draw_frame(&self.pass.gpu, w, h, self.clear, |pass| self.pass.draw(pass, &self.vertices, &self.indices, self.count))
    }
}

/// Glb space (metres, Y up, the preview GLB's) → the game's (Unreal units, Z up): a mirror, so a tangent basis's sign flips.
pub fn from_gltf(pos: Vec3, normal: Vec3, tangent: [f32; 4], uv: [f32; 2]) -> GameVertex {
    let swap = |v: Vec3| Vec3::new(v.x, v.z, v.y);
    GameVertex { pos: swap(pos) * 100.0, normal: swap(normal), tangent: swap(Vec3::new(tangent[0], tangent[1], tangent[2])), sign: -tangent[3], uv }
}

/// The matrix taking the game's space to glb space (the inverse of [`from_gltf`]'s positions).
pub fn to_gltf_matrix() -> Mat4 {
    Mat4::from_cols(glam::Vec4::new(0.01, 0.0, 0.0, 0.0), glam::Vec4::new(0.0, 0.0, 0.01, 0.0), glam::Vec4::new(0.0, 0.01, 0.0, 0.0), glam::Vec4::W)
}

/// Group 0: the vertex constants, the pixel constants and the placements' constants.
fn globals_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, vs: &wgpu::Buffer, ps: &wgpu::Buffer, draw: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: vs.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: ps.as_entire_binding() }, wgpu::BindGroupEntry { binding: 2, resource: draw.as_entire_binding() }],
    })
}

/// One placement's block of per-draw constants: its rows where the shader reads each constant.
fn placement_block(slots: &[(String, usize, usize)], p: &Placement, stride: usize) -> Vec<u8> {
    let mut block = vec![0u8; stride];
    for (name, slot, count) in slots {
        let rows = match name.as_str() {
            "localtoworld" => 0..4,
            "worldtolocal" => 4..7,
            "localtoworldrotdeterminantflip" => 7..8,
            _ => continue,
        };
        let rows = rows.start..rows.start + (*count).min(rows.len());
        let src: &[u8] = bytemuck::cast_slice(&p[rows]);
        if let Some(dst) = block.get_mut(slot * 16..slot * 16 + src.len()) {
            dst.copy_from_slice(src);
        }
    }
    block
}

/// A sampled texture: `images` with their mips (one for 2D, six faces for a
/// cube, colour or data), else white.
fn texture_view(gpu: &Gpu, images: Vec<image::RgbaImage>, srgb: bool, dim: wgpu::TextureViewDimension, filterable: bool) -> wgpu::TextureView {
    let format = if srgb && !images.is_empty() && filterable { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
    let white = || image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]));
    let (layers, dimension): (Vec<Vec<image::RgbaImage>>, wgpu::TextureDimension) = match dim {
        wgpu::TextureViewDimension::D2 => (vec![mips(images.into_iter().next().unwrap_or_else(white))], wgpu::TextureDimension::D2),
        wgpu::TextureViewDimension::Cube if images.len() == 6 => (images.into_iter().map(mips).collect(), wgpu::TextureDimension::D2),
        wgpu::TextureViewDimension::Cube => (vec![vec![white()]; 6], wgpu::TextureDimension::D2),
        wgpu::TextureViewDimension::D3 => (vec![vec![white()]], wgpu::TextureDimension::D3),
        _ => (vec![vec![white()]], wgpu::TextureDimension::D2),
    };
    let levels = &layers[0];
    let (w, h) = (levels[0].width(), levels[0].height());
    let mut data = Vec::new();
    for layer in &layers {
        for l in layer {
            data.extend_from_slice(l.as_raw());
        }
    }
    let layers_n = layers.len() as u32;
    let tex = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers_n }, mip_level_count: levels.len() as u32, sample_count: 1, dimension, format, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dim), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_land_on_their_registers() {
        let list = serde_json::json!([{ "name": "LocalToWorld", "register": 2, "count": 4 }, { "name": "WorldToLocal", "register": 6, "count": 3 }]);
        let mut c = Constants::new(&list, 9);
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        c.set("localtoworld", &m.to_cols_array_2d());
        c.set("WorldToLocal", &Mat4::IDENTITY.to_cols_array_2d());
        c.set("Unknown", &[[9.0; 4]]);
        // Row 3 of the row-vector matrix is the translation, in register 2 + 3.
        assert_eq!(c.data[5], [1.0, 2.0, 3.0, 1.0]);
        // A 3-register constant takes three rows of four.
        assert_eq!(c.data[8], [0.0, 0.0, 1.0, 0.0]);
        assert!(c.data.iter().all(|r| *r != [9.0; 4]));
    }

    #[test]
    fn the_sphere_feeds_the_local_vertex_factory() {
        let inputs = ["POSITION0", "TANGENT0", "NORMAL0", "TEXCOORD0"];
        let (vertices, indices) = sphere();
        let data: Vec<[f32; 4]> = vertices.iter().flat_map(|v| inputs.iter().map(|s| vertex_input(s, v, false))).collect();
        // A point on the equator, facing +X: the position at the radius, the normal packed outward.
        let k = (24 * 97) * 4;
        let p = Vec3::new(data[k][0], data[k][1], data[k][2]);
        assert!((p.length() - RADIUS).abs() < 1e-3);
        assert!((data[k + 2][0] - 255.0).abs() < 1e-3);
        assert_eq!(data[k + 3][3], 1.0);
        // The shader's binormal is cross(N, T) * sign: here N × T = +Z while V runs down, so the sign is negative.
        let unpack = |v: [f32; 4]| Vec3::new(v[0] / 127.5 - 1.0, v[1] / 127.5 - 1.0, v[2] / 127.5 - 1.0);
        let (t, n) = (unpack(data[k + 1]), unpack(data[k + 2]));
        let sign = data[k + 2][3] / 127.5 - 1.0;
        assert!((n.cross(t) * sign - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-2);
        // Every triangle faces out (clockwise seen from outside in a left-handed world).
        let pos = |i: u32| Vec3::new(data[i as usize * 4][0], data[i as usize * 4][1], data[i as usize * 4][2]);
        for t in indices.chunks(3) {
            let n = (pos(t[1]) - pos(t[0])).cross(pos(t[2]) - pos(t[0]));
            assert!(n.dot(pos(t[0]) + pos(t[1]) + pos(t[2])) >= -1e-3);
        }
    }

    #[test]
    fn glb_space_converts_back_to_the_game() {
        // The preview GLB writes (x, z, y) / 100 and negates the tangent sign.
        let v = from_gltf(Vec3::new(1.0, 2.0, 3.0), Vec3::Y, [1.0, 0.0, 0.0, -1.0], [0.5, 0.5]);
        assert_eq!(v.pos, Vec3::new(100.0, 300.0, 200.0));
        assert_eq!(v.normal, Vec3::Z);
        assert_eq!(v.sign, 1.0);
        assert!((to_gltf_matrix().transform_point3(v.pos) - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-5);
    }

    #[test]
    fn blend_modes_follow_the_material() {
        assert_eq!(Blend::of("blend_masked"), Blend::Opaque);
        assert_eq!(Blend::of("BLEND_Translucent"), Blend::Translucent);
        assert_eq!(Blend::of("blend_additive"), Blend::Additive);
        assert_eq!(Blend::of(""), Blend::Opaque);
    }
}
