//! The map view's renderer: every level's actors as nodes carrying their
//! world matrix (view space: metres, Y up, the `map.scene` matrices) and
//! what they draw: meshes (one upload per `mesh.preview` GLB, instanced for
//! every placement), brushes (see-through fills and their edges), lines
//! (cylinders, arrows, light reaches) and markers (small built-in shapes in
//! a flat colour). Drawn off-screen with wgpu like the mesh preview, with a
//! reversed depth buffer (the maps span centimetres to kilometres), and
//! picked on the CPU (the meshes keep their triangles).
//!
//! Draw order: opaque meshes, see-through fills, lines, then what is drawn
//! over everything (X-ray, the gizmo).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use glam::{Mat4, Vec3, Vec4};
use gpui::RenderImage;
use wgpu::util::DeviceExt;

use crate::mesh_view::{gpu, Gpu, Vertex, DEPTH, FORMAT, SAMPLES};

/// A coloured vertex of a line or a see-through triangle.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug)]
pub struct Cv {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

impl Cv {
    pub fn new(p: Vec3, color: [f32; 4]) -> Cv {
        Cv { pos: p.into(), color }
    }
}

/// One placement of a mesh: its matrix, a colour, what the fragment does.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inst {
    m: [[f32; 4]; 4],
    color: [f32; 4],
    /// x: 0 shaded (texture × colour), 1 by slope (heavy walls), 2 flat colour.
    mode: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    /// x: 0 lit, 1 unlit, 2 untextured.
    shading: [f32; 4],
}

const MESH_SHADER: &str = r#"
struct G { view_proj: mat4x4<f32>, eye: vec4<f32>, shading: vec4<f32> };
@group(0) @binding(0) var<uniform> g: G;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) mode: f32,
    @location(4) world: vec3<f32>,
};

@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>, @location(4) m1: vec4<f32>, @location(5) m2: vec4<f32>, @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>, @location(8) mode: vec4<f32>) -> VOut {
    let m = mat4x4<f32>(m0, m1, m2, m3);
    let w = m * vec4<f32>(pos, 1.0);
    var o: VOut;
    o.pos = g.view_proj * w;
    o.world = w.xyz;
    o.n = (m * vec4<f32>(normal, 0.0)).xyz;
    o.uv = uv;
    o.color = color;
    o.mode = mode.x;
    return o;
}

@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    var n = normalize(i.n);
    // Two-sided: cooked meshes are often single-sided cards.
    if (dot(n, g.eye.xyz - i.world) < 0.0) { n = -n; }
    if (i.mode > 1.5) { return i.color; }
    if (i.mode > 0.5) {
        // Can't be stood on: walls (the heavy walls) red, slopes orange, roofs and floors grey.
        let up = abs(n.y);
        var c = vec3<f32>(0.58, 0.60, 0.70);
        if (up < 0.35) { c = vec3<f32>(0.95, 0.30, 0.40); } else if (up < 0.75) { c = vec3<f32>(0.98, 0.60, 0.30); }
        let light = 0.55 + 0.45 * abs(dot(n, normalize(vec3<f32>(0.4, 0.8, 0.3))));
        return vec4<f32>(c * light, 1.0);
    }
    var base = t.rgb * i.color.rgb;
    if (g.shading.x > 1.5) { base = vec3<f32>(0.47, 0.52, 0.99); }
    if (g.shading.x > 0.5 && g.shading.x < 1.5) { return vec4<f32>(base, 1.0); }
    // A hemisphere (sky above, the theme's grey below) and a sun, like the Electron view.
    let sky = mix(vec3<f32>(0.06, 0.06, 0.09), vec3<f32>(1.0, 1.0, 1.0), n.y * 0.5 + 0.5) * 0.62;
    let sun = max(dot(n, normalize(vec3<f32>(30.0, 60.0, 20.0))), 0.0) * 0.62;
    return vec4<f32>(base * (sky + vec3<f32>(sun)), 1.0);
}
"#;

const COLOR_SHADER: &str = r#"
struct G { view_proj: mat4x4<f32>, eye: vec4<f32>, shading: vec4<f32> };
@group(0) @binding(0) var<uniform> g: G;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = g.view_proj * vec4<f32>(pos, 1.0);
    o.color = color;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return i.color; }
"#;

/// The map's pipelines, made once.
struct MapGpu {
    globals_layout: wgpu::BindGroupLayout,
    /// Opaque meshes; wireframe when the GPU draws lines; translucent with the depth test (collision) and over everything (X-ray).
    mesh: wgpu::RenderPipeline,
    mesh_wire: Option<wgpu::RenderPipeline>,
    mesh_trans: wgpu::RenderPipeline,
    mesh_over: wgpu::RenderPipeline,
    /// Coloured triangles (fills) and lines, depth tested or over everything.
    fill: wgpu::RenderPipeline,
    fill_over: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    lines_over: wgpu::RenderPipeline,
    /// Meshes without a texture: the Electron view's grey.
    grey: wgpu::BindGroup,
}

static MAP_GPU: OnceLock<Result<Arc<MapGpu>, String>> = OnceLock::new();

fn map_gpu() -> Result<(Arc<Gpu>, Arc<MapGpu>), String> {
    let g = gpu()?;
    let m = MAP_GPU.get_or_init(|| Ok(Arc::new(MapGpu::new(&g)))).clone()?;
    Ok((g, m))
}

impl MapGpu {
    fn new(g: &Gpu) -> MapGpu {
        let device = &g.device;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("map-mesh"), source: wgpu::ShaderSource::Wgsl(MESH_SHADER.into()) });
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("map-color"), source: wgpu::ShaderSource::Wgsl(COLOR_SHADER.into()) });
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&globals_layout), Some(&g.material_layout)], immediate_size: 0 });
        let color_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&globals_layout)], immediate_size: 0 });
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let depth = |write: bool, test: bool| {
            Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(write),
                // Reversed depth: nearer is larger.
                depth_compare: Some(if test { wgpu::CompareFunction::GreaterEqual } else { wgpu::CompareFunction::Always }),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let inst_attrs = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
        let mesh = |mode: wgpu::PolygonMode, blend: Option<wgpu::BlendState>, depth: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("map-mesh"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_shader,
                    entry_point: Some("vs"),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] }),
                        Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Inst>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs }),
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState { module: &mesh_shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: wgpu::PrimitiveState { polygon_mode: mode, ..Default::default() },
                depth_stencil: depth,
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };
        let colored = |topology: wgpu::PrimitiveTopology, depth: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("map-color"),
                layout: Some(&color_layout),
                vertex: wgpu::VertexState {
                    module: &color_shader,
                    entry_point: Some("vs"),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Cv>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4] })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState { module: &color_shader, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: depth,
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };
        let lines_ok = device.features().contains(wgpu::Features::POLYGON_MODE_LINE);
        let grey_tex = device.create_texture_with_data(
            &g.queue,
            &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[0x93, 0x99, 0xb2, 255],
        );
        let grey_view = grey_tex.create_view(&Default::default());
        let grey = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g.material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&grey_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&g.sampler) }] });
        MapGpu {
            mesh: mesh(wgpu::PolygonMode::Fill, None, depth(true, true)),
            mesh_wire: lines_ok.then(|| mesh(wgpu::PolygonMode::Line, None, depth(true, true))),
            mesh_trans: mesh(wgpu::PolygonMode::Fill, blend, depth(false, true)),
            mesh_over: mesh(wgpu::PolygonMode::Fill, blend, depth(false, false)),
            fill: colored(wgpu::PrimitiveTopology::TriangleList, depth(false, true)),
            fill_over: colored(wgpu::PrimitiveTopology::TriangleList, depth(false, false)),
            lines: colored(wgpu::PrimitiveTopology::LineList, depth(false, true)),
            lines_over: colored(wgpu::PrimitiveTopology::LineList, depth(false, false)),
            globals_layout,
            grey,
        }
    }
}

// ----- meshes ------------------------------------------------------------------

/// A GLB read on a background thread: each primitive's vertices (in the
/// GLB's space), indices and base colour texture.
pub struct CpuMesh {
    pub parts: Vec<(Vec<Vertex>, Vec<u32>, Option<image::RgbaImage>)>,
}

/// Read a `mesh.preview` GLB (no skinning: the map draws meshes as placed).
pub fn parse_glb(glb: &[u8]) -> Result<CpuMesh, String> {
    let gltf = gltf::Gltf::from_slice(glb).map_err(|e| format!("GLB: {e}"))?;
    let blob = gltf.blob.as_deref().unwrap_or_default();
    let buffer_data = |b: gltf::Buffer| -> Option<&[u8]> { matches!(b.source(), gltf::buffer::Source::Bin).then_some(blob) };
    let images: Vec<Option<image::RgbaImage>> = gltf
        .images()
        .map(|img| match img.source() {
            gltf::image::Source::View { view, .. } => blob.get(view.offset()..view.offset() + view.length()).and_then(|png| image::load_from_memory(png).ok()).map(|i| i.to_rgba8()),
            _ => None,
        })
        .collect();
    let scene = gltf.default_scene().or_else(|| gltf.scenes().next()).ok_or("GLB without a scene")?;
    let mut parts = Vec::new();
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        stack.extend(node.children().map(|c| (c, world)));
        let Some(mesh) = node.mesh() else { continue };
        let normal_m = world.inverse().transpose();
        for prim in mesh.primitives() {
            let reader = prim.reader(buffer_data);
            let Some(pos) = reader.read_positions() else { continue };
            let pos: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from(p))).collect();
            let normals: Vec<Vec3> = reader.read_normals().map(|n| n.map(|n| normal_m.transform_vector3(Vec3::from(n)).normalize_or_zero()).collect()).unwrap_or_else(|| vec![Vec3::Y; pos.len()]);
            let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0; 2]; pos.len()]);
            let indices: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
            let vertices = (0..pos.len()).map(|k| Vertex { pos: pos[k].into(), normal: normals.get(k).copied().unwrap_or(Vec3::Y).into(), uv: uvs.get(k).copied().unwrap_or_default() }).collect();
            let texture = prim.material().pbr_metallic_roughness().base_color_texture().and_then(|t| images.get(t.texture().source().index()).cloned().flatten());
            parts.push((vertices, indices, texture));
        }
    }
    if parts.is_empty() {
        return Err("the mesh has no geometry".into());
    }
    Ok(CpuMesh { parts })
}

struct GpuPrim {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    material: wgpu::BindGroup,
}

/// A mesh on the GPU, with its triangles kept for picking.
pub struct GpuMesh {
    prims: Vec<GpuPrim>,
    pub lo: Vec3,
    pub hi: Vec3,
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    /// Feature edges (creases over 20° and borders), made when first asked for.
    edges: OnceLock<Vec<(Vec3, Vec3)>>,
}

impl GpuMesh {
    fn upload(g: &Gpu, m: &MapGpu, cpu: &CpuMesh) -> GpuMesh {
        let device = &g.device;
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let (mut positions, mut triangles) = (Vec::new(), Vec::new());
        let mut prims = Vec::new();
        for (vertices, indices, texture) in &cpu.parts {
            let base = positions.len() as u32;
            for v in vertices {
                let p = Vec3::from(v.pos);
                lo = lo.min(p);
                hi = hi.max(p);
                positions.push(p);
            }
            triangles.extend(indices.as_chunks::<3>().0.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
            let material = match texture {
                Some(t) => {
                    let tex = device.create_texture_with_data(
                        &g.queue,
                        &wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: t.width(), height: t.height(), depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                        wgpu::util::TextureDataOrder::LayerMajor,
                        t.as_raw(),
                    );
                    let view = tex.create_view(&Default::default());
                    device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &g.material_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&g.sampler) }] })
                }
                None => m.grey.clone(),
            };
            prims.push(GpuPrim {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(vertices), usage: wgpu::BufferUsages::VERTEX }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(indices), usage: wgpu::BufferUsages::INDEX }),
                count: indices.len() as u32,
                material,
            });
        }
        if positions.is_empty() {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        GpuMesh { prims, lo, hi, positions, triangles, edges: OnceLock::new() }
    }

    fn edges(&self) -> &[(Vec3, Vec3)] {
        self.edges.get_or_init(|| {
            // Edges shared by faces bending more than 20°, and borders (three's EdgesGeometry).
            let key = |p: Vec3| ((p.x * 1e4) as i64, (p.y * 1e4) as i64, (p.z * 1e4) as i64);
            // By its two ends (rounded, either order): the edge, its first face's normal and the second's.
            type Ends = ((i64, i64, i64), (i64, i64, i64));
            let mut map: HashMap<Ends, (Vec3, Vec3, Vec3, Option<Vec3>)> = HashMap::new();
            for t in &self.triangles {
                let p = [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]];
                let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
                for k in 0..3 {
                    let (a, b) = (p[k], p[(k + 1) % 3]);
                    let (ka, kb) = (key(a), key(b));
                    let id = if ka < kb { (ka, kb) } else { (kb, ka) };
                    map.entry(id).and_modify(|e| e.3 = Some(n)).or_insert((a, b, n, None));
                }
            }
            let cos = 20f32.to_radians().cos();
            map.into_values().filter(|(_, _, n, m)| m.is_none_or(|m| n.dot(m) < cos)).map(|(a, b, ..)| (a, b)).collect()
        })
    }

    /// The nearest triangle hit by a ray in the mesh's space.
    fn ray_hit(&self, o: Vec3, d: Vec3) -> Option<f32> {
        let mut best: Option<f32> = None;
        for t in &self.triangles {
            let (a, b, c) = (self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]);
            if let Some(h) = ray_triangle(o, d, a, b, c) {
                if best.is_none_or(|x| h < x) {
                    best = Some(h);
                }
            }
        }
        best
    }
}

/// Möller–Trumbore, both sides.
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// Where a ray enters an axis-aligned box (`None` when it misses).
fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let inv = Vec3::ONE / d;
    let (t1, t2) = ((lo - o) * inv, (hi - o) * inv);
    let (tmin, tmax) = (t1.min(t2).max_element(), t1.max(t2).min_element());
    (tmax >= tmin.max(0.0)).then_some(tmin.max(0.0))
}

/// The distance between a ray and a segment, and how far along the ray.
fn ray_segment(o: Vec3, d: Vec3, a: Vec3, b: Vec3) -> (f32, f32) {
    let u = b - a;
    let w = o - a;
    let (aa, bb, cc, dd, ee) = (d.dot(d), d.dot(u), u.dot(u), d.dot(w), u.dot(w));
    let den = aa * cc - bb * bb;
    // The closest point of the segment's line, kept on the segment, then the ray's point nearest it.
    let t = if den.abs() < 1e-12 { ee / cc.max(1e-12) } else { (aa * ee - bb * dd) / den }.clamp(0.0, 1.0);
    let s = ((a + u * t - o).dot(d) / aa.max(1e-12)).max(0.0);
    ((o + d * s - (a + u * t)).length(), s)
}

// ----- built-in shapes ------------------------------------------------------------

/// Markers of actors that draw nothing themselves.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Shape {
    Octahedron,
    Tetrahedron,
    Sphere,
    Box,
}

impl Shape {
    fn key(self) -> String {
        format!("@{self:?}")
    }

    fn mesh(self) -> CpuMesh {
        let (pos, tris): (Vec<Vec3>, Vec<[u32; 3]>) = match self {
            Shape::Octahedron => (
                vec![Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z],
                vec![[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5]],
            ),
            Shape::Tetrahedron => (vec![Vec3::new(1.0, 1.0, 1.0), Vec3::new(-1.0, -1.0, 1.0), Vec3::new(-1.0, 1.0, -1.0), Vec3::new(1.0, -1.0, -1.0)].into_iter().map(|v| v.normalize()).collect(), vec![[2, 1, 0], [0, 3, 2], [1, 3, 0], [2, 3, 1]]),
            Shape::Box => {
                let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * 0.5;
                (
                    vec![c(-1., -1., -1.), c(1., -1., -1.), c(1., 1., -1.), c(-1., 1., -1.), c(-1., -1., 1.), c(1., -1., 1.), c(1., 1., 1.), c(-1., 1., 1.)],
                    vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 6, 2], [3, 7, 6], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]],
                )
            }
            Shape::Sphere => {
                let (rings, segs) = (6u32, 8u32);
                let mut pos = Vec::new();
                for r in 0..=rings {
                    let phi = r as f32 / rings as f32 * std::f32::consts::PI;
                    for s in 0..=segs {
                        let th = s as f32 / segs as f32 * std::f32::consts::TAU;
                        pos.push(Vec3::new(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin()));
                    }
                }
                let mut tris = Vec::new();
                for r in 0..rings {
                    for s in 0..segs {
                        let a = r * (segs + 1) + s;
                        let b = a + segs + 1;
                        tris.push([a, b, a + 1]);
                        tris.push([a + 1, b, b + 1]);
                    }
                }
                (pos, tris)
            }
        };
        // Flat faces: every triangle its own corners and normal.
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for t in tris {
            let p = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            for q in p {
                indices.push(vertices.len() as u32);
                vertices.push(Vertex { pos: q.into(), normal: n.into(), uv: [0.0; 2] });
            }
        }
        CpuMesh { parts: vec![(vertices, indices, None)] }
    }
}

// ----- the scene ------------------------------------------------------------------------

/// What a node draws, in the node's space.
#[derive(Clone)]
pub enum Part {
    /// A loaded (or loading) mesh, `color` multiplying its texture (`flat`: one colour, unlit).
    Mesh { key: String, local: Mat4, color: [f32; 4], flat: bool, edges: Option<[f32; 4]> },
    /// A built-in shape in a flat colour.
    Marker { shape: Shape, local: Mat4, color: [f32; 4] },
    /// Line segments (pairs of vertices); `pick`: clicks on them select the node.
    Lines { verts: Vec<Cv>, pick: bool },
    /// See-through triangles; `brush`: a volume's fill (heavy walls colour it by slope).
    Fill { verts: Vec<Cv>, brush: bool },
}

/// An actor (or a collision item, a nav mesh): its world matrix and what it draws.
#[derive(Clone)]
pub struct Node {
    pub matrix: Mat4,
    pub visible: bool,
    /// `m_bCanBaseOn` False: drawn by slope (or in X-ray) when heavy walls are on.
    pub rooftop: bool,
    /// A light's colour over its markers (Matinee tints), RGB 0..1.
    pub tint: Option<[f32; 3]>,
    pub parts: Vec<Part>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Shading {
    #[default]
    Lit,
    Unlit,
    Untextured,
    Wireframe,
}

impl Shading {
    pub fn key(self) -> &'static str {
        match self {
            Shading::Lit => "lit",
            Shading::Unlit => "unlit",
            Shading::Untextured => "untextured",
            Shading::Wireframe => "wireframe",
        }
    }

    pub fn parse(s: &str) -> Option<Shading> {
        Some(match s {
            "lit" => Shading::Lit,
            "unlit" => Shading::Unlit,
            "untextured" => Shading::Untextured,
            "wireframe" => Shading::Wireframe,
            _ => return None,
        })
    }
}

/// How flagged actors are drawn: faces by slope, or one colour seen through walls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Heavy {
    Slope,
    Xray,
}

/// The fly/orbit camera: where it is and the point it turns around.
#[derive(Clone, Copy, Debug)]
pub struct Cam {
    pub eye: Vec3,
    pub target: Vec3,
}

impl Cam {
    pub const FOV: f32 = 60.0;

    pub fn view_proj(&self, w: f32, h: f32) -> Mat4 {
        let proj = Mat4::perspective_infinite_reverse_rh(Self::FOV.to_radians(), (w / h.max(1.0)).max(0.01), 0.05);
        proj * Mat4::look_at_rh(self.eye, self.target, Vec3::Y)
    }

    /// The ray under a point of a `w`×`h` view.
    pub fn ray(&self, x: f32, y: f32, w: f32, h: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(w, h).inverse();
        let ndc = Vec4::new(x / w * 2.0 - 1.0, 1.0 - y / h * 2.0, 1.0, 1.0);
        let near = inv * ndc;
        let near = near.truncate() / near.w;
        (self.eye, (near - self.eye).normalize_or(Vec3::NEG_Z))
    }

    /// Where a point is in a `w`×`h` view (`None` behind the camera).
    pub fn project(&self, p: Vec3, w: f32, h: f32) -> Option<(f32, f32)> {
        let c = self.view_proj(w, h) * p.extend(1.0);
        (c.w > 1e-6).then(|| ((c.x / c.w * 0.5 + 0.5) * w, (0.5 - c.y / c.w * 0.5) * h))
    }

    /// The camera's forward and right directions.
    pub fn axes(&self) -> (Vec3, Vec3) {
        let f = (self.target - self.eye).normalize_or(Vec3::NEG_Z);
        (f, f.cross(Vec3::Y).normalize_or(Vec3::X))
    }
}

enum Slot {
    Loading,
    Failed,
    Ready(Arc<GpuMesh>),
}

/// Built from the nodes once and reused until something changes.
struct Built {
    instances: Option<wgpu::Buffer>,
    /// Per mesh: its key and the range of instances (opaque, translucent, X-ray).
    batches: Vec<(Arc<GpuMesh>, std::ops::Range<u32>, u8)>,
    fill: Option<(wgpu::Buffer, u32)>,
    fill_over: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
    lines_over: Option<(wgpu::Buffer, u32)>,
}

/// The scene: layers of nodes, the meshes they use, and how they are drawn.
pub struct MapScene {
    gpu: Arc<Gpu>,
    m: Arc<MapGpu>,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    meshes: HashMap<String, Slot>,
    /// The actors, by layer: 0 the map, 1 collision, 2 the nav mesh.
    pub layers: [Vec<Node>; 3],
    /// Layers drawn (the map is hidden with "collision only").
    pub shown: [bool; 3],
    /// The nav mesh's layer is drawn over everything.
    pub nav_xray: bool,
    pub cam: Cam,
    pub shading: Shading,
    pub heavy: Option<Heavy>,
    pub clear: [f64; 3],
    /// Lines drawn each frame over the scene, depth tested (selection, paths) or not (the gizmo).
    pub overlay: Vec<Cv>,
    pub overlay_top: Vec<Cv>,
    built: Option<Built>,
}

impl MapScene {
    pub fn new() -> Result<MapScene, String> {
        let (g, m) = map_gpu()?;
        let globals = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &m.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }] });
        let mut s = MapScene {
            gpu: g,
            m,
            globals,
            globals_group,
            meshes: HashMap::new(),
            layers: [Vec::new(), Vec::new(), Vec::new()],
            shown: [true, true, true],
            nav_xray: false,
            cam: Cam { eye: Vec3::new(40.0, 40.0, 40.0), target: Vec3::ZERO },
            shading: Shading::Lit,
            heavy: None,
            clear: [0.009, 0.009, 0.016],
            overlay: Vec::new(),
            overlay_top: Vec::new(),
            built: None,
        };
        for shape in [Shape::Octahedron, Shape::Tetrahedron, Shape::Sphere, Shape::Box] {
            s.add_mesh(shape.key(), &shape.mesh());
        }
        Ok(s)
    }

    /// Something the nodes draw changed (visibility, matrices, parts, colours): rebuild before the next frame.
    pub fn invalidate(&mut self) {
        self.built = None;
    }

    /// Whether a mesh is known (loaded, loading or failed).
    pub fn has_mesh(&self, key: &str) -> bool {
        self.meshes.contains_key(key)
    }

    pub fn mark_loading(&mut self, key: String) {
        self.meshes.entry(key).or_insert(Slot::Loading);
    }

    pub fn mark_failed(&mut self, key: String) {
        self.meshes.insert(key, Slot::Failed);
        self.built = None;
    }

    pub fn add_mesh(&mut self, key: String, cpu: &CpuMesh) {
        self.meshes.insert(key, Slot::Ready(Arc::new(GpuMesh::upload(&self.gpu, &self.m, cpu))));
        self.built = None;
    }

    /// Meshes loading, loaded and failed.
    pub fn mesh_counts(&self) -> (usize, usize, usize) {
        let mut out = (0, 0, 0);
        for (k, s) in &self.meshes {
            if k.starts_with('@') {
                continue;
            }
            match s {
                Slot::Loading => out.0 += 1,
                Slot::Ready(_) => out.1 += 1,
                Slot::Failed => out.2 += 1,
            }
        }
        out
    }

    fn mesh(&self, key: &str) -> Option<&Arc<GpuMesh>> {
        match self.meshes.get(key) {
            Some(Slot::Ready(m)) => Some(m),
            _ => None,
        }
    }

    /// A node's world-space bounds (`None` when it draws nothing measurable).
    pub fn bounds(&self, layer: usize, i: usize) -> Option<(Vec3, Vec3)> {
        let n = self.layers[layer].get(i)?;
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        let mut add = |p: Vec3| {
            lo = lo.min(p);
            hi = hi.max(p);
        };
        let corners = |m: Mat4, a: Vec3, b: Vec3, add: &mut dyn FnMut(Vec3)| {
            for k in 0..8 {
                add(m.transform_point3(Vec3::new(if k & 1 == 0 { a.x } else { b.x }, if k & 2 == 0 { a.y } else { b.y }, if k & 4 == 0 { a.z } else { b.z })));
            }
        };
        for p in &n.parts {
            match p {
                Part::Mesh { key, local, .. } => match self.mesh(key) {
                    Some(m) => corners(n.matrix * *local, m.lo, m.hi, &mut add),
                    None => corners(n.matrix * *local, Vec3::splat(-0.25), Vec3::splat(0.25), &mut add),
                },
                Part::Marker { local, .. } => corners(n.matrix * *local, Vec3::splat(-1.0), Vec3::splat(1.0), &mut add),
                Part::Lines { verts, .. } | Part::Fill { verts, .. } => {
                    for v in verts {
                        add(n.matrix.transform_point3(Vec3::from(v.pos)));
                    }
                }
            }
        }
        (lo.x <= hi.x).then_some((lo, hi))
    }

    /// The nearest node of `layer` under a ray: meshes and markers by their
    /// triangles, lines within a few centimetres (volumes are picked by their
    /// edges, so meshes inside them stay clickable).
    pub fn pick(&self, layer: usize, o: Vec3, d: Vec3) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        let mut take = |i: usize, t: f32| {
            if best.is_none_or(|(_, b)| t < b) {
                best = Some((i, t));
            }
        };
        for (i, n) in self.layers[layer].iter().enumerate() {
            if !n.visible {
                continue;
            }
            for p in &n.parts {
                let solid = match p {
                    Part::Mesh { key, local, .. } => Some((key.clone(), *local)),
                    Part::Marker { shape, local, .. } => Some((shape.key(), *local)),
                    _ => None,
                };
                if let Some((key, local)) = solid {
                    let m = n.matrix * local;
                    let inv = m.inverse();
                    let (lo, ld) = (inv.transform_point3(o), inv.transform_vector3(d));
                    let hit = match self.mesh(&key) {
                        Some(g) => ray_box(lo, ld, g.lo, g.hi).and_then(|_| g.ray_hit(lo, ld)),
                        None => ray_box(lo, ld, Vec3::splat(-0.25), Vec3::splat(0.25)),
                    };
                    if let Some(t) = hit {
                        // Back to a world distance along the ray.
                        take(i, (m.transform_point3(lo + ld * t) - o).dot(d));
                    }
                    continue;
                }
                if let Part::Lines { verts, pick: true } = p {
                    {
                        for s in verts.as_chunks::<2>().0.iter() {
                            let (a, b) = (n.matrix.transform_point3(Vec3::from(s[0].pos)), n.matrix.transform_point3(Vec3::from(s[1].pos)));
                            let (dist, t) = ray_segment(o, d, a, b);
                            // About 0.1 m near, growing with the distance like a few pixels.
                            if dist < 0.1f32.max(t * 0.006) {
                                take(i, t);
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// The nearest triangle of a list of world-space triangles under a ray.
    pub fn pick_triangles(o: Vec3, d: Vec3, positions: &[Vec3], tris: &[[u32; 3]]) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (k, t) in tris.iter().enumerate() {
            if let Some(h) = ray_triangle(o, d, positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]) {
                if best.is_none_or(|(_, b)| h < b) {
                    best = Some((k, h));
                }
            }
        }
        best
    }

    fn build(&self) -> Built {
        let device = &self.gpu.device;
        // Meshes: per mesh and kind (0 opaque, 1 translucent, 2 over everything), every instance.
        let mut groups: HashMap<(String, u8), Vec<Inst>> = HashMap::new();
        let (mut fill, mut fill_over, mut lines, mut lines_over) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let heavy = self.heavy;
        for (layer, nodes) in self.layers.iter().enumerate() {
            if !self.shown[layer] {
                continue;
            }
            let over_layer = layer == 2 && self.nav_xray;
            for n in nodes.iter().filter(|n| n.visible) {
                let flagged = heavy.filter(|_| n.rooftop);
                for p in &n.parts {
                    match p {
                        Part::Mesh { key, local, color, flat, edges } => {
                            let m = n.matrix * *local;
                            let (mode, kind, color) = match flagged {
                                Some(Heavy::Xray) => (2.0, 2u8, [1.0, 0.176, 0.333, 0.6]),
                                Some(Heavy::Slope) => (1.0, 0, *color),
                                None if *flat => (2.0, if color[3] < 1.0 { 1 } else { 0 }, *color),
                                None => (0.0, 0, *color),
                            };
                            // A mesh that could not be loaded: a small red box in its place.
                            let (key, kind, mode, color, m) = if self.mesh(key).is_some() {
                                (key.clone(), kind, mode, color, m)
                            } else if matches!(self.meshes.get(key), Some(Slot::Failed)) {
                                (Shape::Box.key(), 1, 2.0, [0.95, 0.55, 0.66, 0.6], m * Mat4::from_scale(Vec3::splat(0.5)))
                            } else {
                                continue;
                            };
                            groups.entry((key.clone(), kind)).or_default().push(Inst { m: m.to_cols_array_2d(), color, mode: [mode, 0.0, 0.0, 0.0] });
                            if let (Some(c), Some(g)) = (edges, self.mesh(&key)) {
                                for (a, b) in g.edges() {
                                    lines.push(Cv::new(m.transform_point3(*a), *c));
                                    lines.push(Cv::new(m.transform_point3(*b), *c));
                                }
                            }
                        }
                        Part::Marker { shape, local, color } => {
                            let mut c = *color;
                            if let Some(t) = n.tint {
                                c = [t[0], t[1], t[2], c[3]];
                            }
                            let kind = if over_layer { 2 } else { 0 };
                            groups.entry((shape.key(), kind)).or_default().push(Inst { m: (n.matrix * *local).to_cols_array_2d(), color: c, mode: [2.0, 0.0, 0.0, 0.0] });
                        }
                        Part::Lines { verts, .. } => {
                            let out = if over_layer || flagged == Some(Heavy::Xray) { &mut lines_over } else { &mut lines };
                            out.extend(verts.iter().map(|v| Cv::new(n.matrix.transform_point3(Vec3::from(v.pos)), v.color)));
                        }
                        Part::Fill { verts, brush } => {
                            let over = over_layer || (*brush && flagged == Some(Heavy::Xray));
                            let out = if over { &mut fill_over } else { &mut fill };
                            for t in verts.as_chunks::<3>().0.iter() {
                                let w = [0, 1, 2].map(|k| n.matrix.transform_point3(Vec3::from(t[k].pos)));
                                let color = match (brush, flagged) {
                                    (true, Some(Heavy::Xray)) => [1.0, 0.69, 0.125, 0.15],
                                    (true, Some(Heavy::Slope)) => {
                                        let up = (w[1] - w[0]).cross(w[2] - w[0]).normalize_or_zero().y.abs();
                                        let c = if up < 0.35 { [0.95, 0.30, 0.40] } else if up < 0.75 { [0.98, 0.60, 0.30] } else { [0.58, 0.60, 0.70] };
                                        [c[0], c[1], c[2], 0.35]
                                    }
                                    _ => t[0].color,
                                };
                                for (k, p) in w.into_iter().enumerate() {
                                    out.push(Cv::new(p, if matches!((brush, flagged), (true, Some(_))) { color } else { t[k].color }));
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut all = Vec::new();
        let mut batches = Vec::new();
        let mut keys: Vec<_> = groups.into_iter().collect();
        keys.sort_by_key(|((_, kind), _)| *kind);
        for ((key, kind), insts) in keys {
            let Some(m) = self.mesh(&key) else { continue };
            let start = all.len() as u32;
            all.extend(insts);
            batches.push((m.clone(), start..all.len() as u32, kind));
        }
        let buf = |v: &[Cv]| (!v.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(v), usage: wgpu::BufferUsages::VERTEX }), v.len() as u32));
        Built {
            instances: (!all.is_empty()).then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&all), usage: wgpu::BufferUsages::VERTEX })),
            batches,
            fill: buf(&fill),
            fill_over: buf(&fill_over),
            lines: buf(&lines),
            lines_over: buf(&lines_over),
        }
    }

    /// Draw a `w`×`h` frame (device pixels).
    pub fn render(&mut self, w: u32, h: u32) -> Arc<RenderImage> {
        let (w, h) = (w.max(16), h.max(16));
        if self.built.is_none() {
            self.built = Some(self.build());
        }
        let shading = match self.shading {
            Shading::Lit | Shading::Wireframe => 0.0,
            Shading::Unlit => 1.0,
            Shading::Untextured => 2.0,
        };
        let g = Globals { view_proj: self.cam.view_proj(w as f32, h as f32).to_cols_array_2d(), eye: self.cam.eye.extend(1.0).into(), shading: [shading, 0.0, 0.0, 0.0] };
        self.gpu.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        let device = &self.gpu.device;
        let buf = |v: &[Cv]| (!v.is_empty()).then(|| (device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(v), usage: wgpu::BufferUsages::VERTEX }), v.len() as u32));
        let (overlay, top) = (buf(&self.overlay), buf(&self.overlay_top));
        let built = self.built.as_ref().expect("built above");
        let m = &self.m;
        let wire = self.shading == Shading::Wireframe;
        crate::mesh_view::draw_frame_depth(&self.gpu, w, h, self.clear, 0.0, |pass| {
            pass.set_bind_group(0, &self.globals_group, &[]);
            let draw_meshes = |pass: &mut wgpu::RenderPass<'_>, kind: u8, pipeline: &wgpu::RenderPipeline| {
                let Some(inst) = &built.instances else { return };
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(1, inst.slice(..));
                for (mesh, range, k) in &built.batches {
                    if *k != kind {
                        continue;
                    }
                    for p in &mesh.prims {
                        pass.set_bind_group(1, &p.material, &[]);
                        pass.set_vertex_buffer(0, p.vertices.slice(..));
                        pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..p.count, 0, range.clone());
                    }
                }
            };
            let opaque = match (&m.mesh_wire, wire) {
                (Some(p), true) => p,
                _ => &m.mesh,
            };
            draw_meshes(pass, 0, opaque);
            draw_meshes(pass, 1, &m.mesh_trans);
            let colored = |pass: &mut wgpu::RenderPass<'_>, b: &Option<(wgpu::Buffer, u32)>, pipeline: &wgpu::RenderPipeline| {
                if let Some((b, n)) = b {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, b.slice(..));
                    pass.draw(0..*n, 0..1);
                }
            };
            colored(pass, &built.fill, &m.fill);
            colored(pass, &built.lines, &m.lines);
            colored(pass, &overlay, &m.lines);
            draw_meshes(pass, 2, &m.mesh_over);
            colored(pass, &built.fill_over, &m.fill_over);
            colored(pass, &built.lines_over, &m.lines_over);
            colored(pass, &top, &m.lines_over);
        })
    }
}

// ----- geometry helpers (lines and fills in a node's space) ---------------------------

pub fn rgba(hex: u32, a: f32) -> [f32; 4] {
    // Colours are sRGB like the Electron view's CSS; the target is sRGB, so make them linear.
    let lin = |c: u32| {
        let v = c as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [lin((hex >> 16) & 255), lin((hex >> 8) & 255), lin(hex & 255), a]
}

/// A wire box's 12 edges.
pub fn box_lines(lo: Vec3, hi: Vec3, color: [f32; 4], out: &mut Vec<Cv>) {
    let c = |k: usize| Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
    for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
        out.push(Cv::new(c(a), color));
        out.push(Cv::new(c(b), color));
    }
}

/// A circle of `n` segments around `axis` (unit), radius `r`.
pub fn circle(center: Vec3, axis: Vec3, r: f32, n: usize, color: [f32; 4], out: &mut Vec<Cv>) {
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let b = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
        out.push(Cv::new(center + (u * a.cos() + v * a.sin()) * r, color));
        out.push(Cv::new(center + (u * b.cos() + v * b.sin()) * r, color));
    }
}

/// A wire sphere: three great circles.
pub fn sphere_lines(center: Vec3, r: f32, color: [f32; 4], out: &mut Vec<Cv>) {
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        circle(center, axis, r, 32, color, out);
    }
}

/// An open cylinder along Y (three's CylinderGeometry, wireframe): rings and sides.
pub fn cylinder_lines(r: f32, half: f32, color: [f32; 4], out: &mut Vec<Cv>) {
    for y in [-half, half] {
        circle(Vec3::new(0.0, y, 0.0), Vec3::Y, r, 16, color, out);
    }
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        let p = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
        out.push(Cv::new(p - Vec3::Y * half, color));
        out.push(Cv::new(p + Vec3::Y * half, color));
    }
}

/// An arrow along `dir` from `from`, `len` long (ArrowHelper).
pub fn arrow_lines(from: Vec3, dir: Vec3, len: f32, head: f32, color: [f32; 4], out: &mut Vec<Cv>) {
    let tip = from + dir * len;
    out.push(Cv::new(from, color));
    out.push(Cv::new(tip, color));
    let side = dir.any_orthonormal_vector();
    let up = dir.cross(side);
    for s in [side, -side, up, -up] {
        out.push(Cv::new(tip, color));
        out.push(Cv::new(tip - dir * head + s * head * 0.5, color));
    }
}

/// Triangles of an indexed list as a fill and its feature edges as lines.
pub fn solid(positions: &[Vec3], indices: &[u32], fill: [f32; 4], edge: [f32; 4]) -> (Vec<Cv>, Vec<Cv>) {
    let tris: Vec<[u32; 3]> = indices.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect();
    let mut f = Vec::with_capacity(tris.len() * 3);
    for t in &tris {
        for &k in t {
            f.push(Cv::new(positions[k as usize], fill));
        }
    }
    let m = GpuMesh { prims: Vec::new(), lo: Vec3::ZERO, hi: Vec3::ZERO, positions: positions.to_vec(), triangles: tris, edges: OnceLock::new() };
    let mut l = Vec::new();
    for (a, b) in m.edges() {
        l.push(Cv::new(*a, edge));
        l.push(Cv::new(*b, edge));
    }
    (f, l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_what_they_cross() {
        let (a, b, c) = (Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert!((ray_triangle(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, a, b, c).unwrap() - 5.0).abs() < 1e-5);
        assert!(ray_triangle(Vec3::new(3.0, 0.0, 5.0), Vec3::NEG_Z, a, b, c).is_none());
        assert!(ray_box(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, Vec3::splat(-1.0), Vec3::splat(1.0)).is_some());
        let (d, t) = ray_segment(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, Vec3::new(-1.0, 0.05, 0.0), Vec3::new(1.0, 0.05, 0.0));
        assert!((d - 0.05).abs() < 1e-4 && (t - 5.0).abs() < 1e-4);
    }

    #[test]
    fn the_camera_projects_what_it_casts() {
        let cam = Cam { eye: Vec3::new(3.0, 4.0, 5.0), target: Vec3::ZERO };
        let (o, d) = cam.ray(300.0, 200.0, 800.0, 600.0);
        let p = o + d * 7.0;
        let (x, y) = cam.project(p, 800.0, 600.0).unwrap();
        assert!((x - 300.0).abs() < 0.05 && (y - 200.0).abs() < 0.05, "{x} {y}");
    }

    #[test]
    fn a_cube_has_twelve_feature_edges() {
        let m = Shape::Box.mesh();
        let (v, i, _) = &m.parts[0];
        let p: Vec<Vec3> = v.iter().map(|v| Vec3::from(v.pos)).collect();
        let (_, lines) = solid(&p, i, [1.0; 4], [1.0; 4]);
        assert_eq!(lines.len(), 24);
    }
}
