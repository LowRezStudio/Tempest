//! A material's own compiled shaders, for a preview that draws like the
//! game: the material shader map in the `ShaderCache` of its package (or of
//! a package of its parent chain), the base pass pair of the local vertex
//! factory (static meshes) translated to WGSL, the constants the pair reads
//! and the textures its samplers take.
//!
//! Cooked materials keep no expression graph, so there is nothing else to
//! draw them from. The pair is picked by light-map policy: one directional
//! light (with the sky light when the material was compiled with it), else
//! one light with spherical-harmonic ambient (level materials), else no
//! light map (unlit and emissive-only materials).

use std::path::Path;

use tempest_upk::io::{Guid, PackageIndex};
use tempest_upk::Package;

use crate::d3d9::{self, RegisterSet, SamplerKind, Shader, Stage};
use crate::material::StaticParameterSet;
use crate::material_ops::{MaterialInfo, PackageCache};
use crate::object::Native;
use crate::shadercache::{package_caches, CodeStorage, MaterialShaderMap, ShaderCache, UniformExpr};
use crate::value::Value;

/// The vertex factory of static meshes, which the preview's own geometry stands in for.
pub const LOCAL_VERTEX_FACTORY: &str = "FLocalVertexFactory";

/// Light-map policies in the order they are tried: one dynamic directional
/// light (characters, props); one directional light with spherical-harmonic
/// ambient (what level materials, compiled for light maps, have for dynamic
/// lighting); no light map (unlit and effect materials).
const POLICIES: &[&str] = &["FDirectionalLightLightMapPolicy", "FSHLightLightMapPolicy", "FNoLightMapPolicy"];

/// A float constant a stage reads, by its `CTAB` name.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatConstant {
    pub name: String,
    pub register: u16,
    pub count: u16,
    /// Where it is in the stage's flat array, in bytes, and how many (SM5 packs scalars into a register's components).
    pub offset: u32,
    pub bytes: u32,
}

/// What a sampler binds.
#[derive(Clone, Debug, PartialEq)]
pub enum SamplerSource {
    /// A material texture: (package, export).
    Texture(String, usize),
    /// A material texture that could not be found (its description).
    Missing(String),
    /// A cube map of the material (not decoded: the preview binds a neutral one).
    Cube,
    /// The renderer's own (light attenuation, shadows, light maps, scene colour…): the preview binds a neutral one.
    Renderer,
}

/// One sampler of a stage, in the module's binding order.
#[derive(Clone, Debug)]
pub struct Sampler {
    pub register: u16,
    pub name: String,
    /// The material's texture expression it samples: its parameter name, or its index in the material's textures.
    pub expression: Option<String>,
    pub kind: SamplerKind,
    pub source: SamplerSource,
}

/// The translated pair and what it needs.
#[derive(Clone, Debug)]
pub struct GameShader {
    pub friendly_name: String,
    pub vertex_factory: String,
    pub vertex_type: String,
    pub pixel_type: String,
    pub module: d3d9::Module,
    pub vertex_constants: Vec<FloatConstant>,
    pub pixel_constants: Vec<FloatConstant>,
    pub pixel_vectors: Vec<UniformExpr>,
    pub pixel_scalars: Vec<UniformExpr>,
    pub vertex_vectors: Vec<UniformExpr>,
    pub vertex_scalars: Vec<UniformExpr>,
    pub samplers: Vec<Sampler>,
    pub vertex_samplers: Vec<Sampler>,
    /// The other vertex factories the material was compiled for.
    pub other_factories: Vec<String>,
    /// `sm3` or `sm5`: what the module was translated from, and why not SM5 when it was asked for.
    pub platform: &'static str,
    pub fallback: Option<String>,
}

/// The resource the shader map was compiled from: the nearest object of the
/// chain with its own (an instance with a static permutation, else the base
/// material), and the package it is in.
struct Resource {
    package: String,
    id: Guid,
    textures: Vec<PackageIndex>,
    /// An instance's static parameters: the key of its shader map.
    statics: Option<Key>,
}

/// A static parameter set compared across packages: the base material and
/// every switch, mask and normal setting by name (lower case).
#[derive(Clone, Debug, PartialEq)]
struct Key {
    base: Guid,
    params: Vec<(String, i32)>,
}

impl Key {
    fn of(set: &StaticParameterSet, pkg: &Package) -> Key {
        let name = |n| pkg.fname_to_string(n).to_ascii_lowercase();
        let mut params: Vec<(String, i32)> = set.static_switch_parameters.iter().map(|p| (name(p.name), p.value)).collect();
        params.extend(set.static_component_mask_parameters.iter().map(|p| (format!("{}#mask", name(p.name)), p.r | p.g << 1 | p.b << 2 | p.a << 3)));
        params.extend(set.normal_parameters.iter().map(|p| (format!("{}#normal", name(p.name)), p.compression_settings as i32)));
        params.sort();
        Key { base: set.base_material_id, params }
    }
}

fn own_resource(cache: &mut PackageCache, info: &MaterialInfo) -> Option<Resource> {
    for (pname, path, _) in &info.chain {
        let idx = cache.get(pname)?.find_export(path)?;
        let obj = cache.object(pname, idx)?;
        // The highest quality (0) first.
        let res = match &obj.native {
            Native::MaterialInstance(m) => m.permutations.iter().min_by_key(|p| p.0).map(|p| (&p.1, Some(&p.2))),
            Native::Material(m) => m.resources.iter().min_by_key(|r| r.0).map(|r| (&r.1, None)),
            _ => None,
        };
        if let Some((r, statics)) = res {
            let pkg = cache.get(pname)?.pkg.clone();
            let statics = statics.map(|s| Key::of(s, &pkg));
            return Some(Resource { package: pname.clone(), id: r.id, textures: r.uniform_expression_textures.clone(), statics });
        }
    }
    None
}

/// The shader map of `res` in `pkg`'s Direct3D 9 shader caches. Maps are
/// found by key (`FMaterialShaderMap::FindId`): instances with the same
/// static parameters share one, which keeps the id of the first compiled.
fn find_map(pkg: &Package, res: &Resource) -> Option<(MaterialShaderMap, Vec<ShaderCache>)> {
    let caches: Vec<ShaderCache> = package_caches(pkg).into_iter().filter_map(Result::ok).collect();
    let map = caches.iter().filter(|c| c.platform == 0).flat_map(|c| c.material_maps.iter()).filter(|m| m.platform == 0).find(|m| match &res.statics {
        Some(key) => Key::of(&m.key, pkg) == *key,
        None => m.material_id == res.id || m.key.base_material_id == res.id && m.key.static_switch_parameters.is_empty(),
    })?;
    Some((map.clone(), caches))
}

fn float_constants(sh: &Shader) -> Vec<FloatConstant> {
    sh.constants.iter().filter(|c| c.set == RegisterSet::Float).map(|c| FloatConstant { name: c.name.clone(), register: c.register, count: c.count, offset: c.register as u32 * 16, bytes: c.count as u32 * 16 }).collect()
}

/// A float constant of an SM5 shader: name, constant buffer, byte offset, bytes.
pub type PairedFloat = (String, u32, u32, u32);
/// A texture of an SM5 shader: the SM3 sampler register it is, its SM5 texture slot.
pub type PairedTexture = (u16, u32);

/// The bindings of a shader model 5 shader, named through its shader model 3
/// twin (the same shader type of the same material). Both records serialise
/// the type's parameters in the same order (`FShaderParameter` BaseIndex,
/// NumBytes, BufferIndex; `FShaderResourceParameter` BaseIndex, NumResources,
/// SamplerIndex), so a parameter found in the SM3 record by its `CTAB`
/// register sits at the same place in the SM5 one. Returns the float
/// constants (name, buffer, byte offset, bytes) and the textures (SM3
/// sampler register, SM5 texture slot).
pub fn pair_parameters(sm3: &[u8], sm5: &[u8], ctab: &Shader, sh5: &crate::dxbc::Shader) -> (Vec<PairedFloat>, Vec<PairedTexture>) {
    // What the SM5 shader declares: a pair must fit it (the records also hold small integers, array indices, that look like parameters).
    let buffers = sh5.constant_buffers();
    let textures5: Vec<u32> = crate::dxbc::textures(sh5).iter().map(|t| t.0).collect();
    let samplers5: Vec<u32> = sh5.instructions.iter().filter(|i| i.opcode == crate::dxbc::opcode::DCL_SAMPLER).filter_map(|i| match i.operands.first()?.indices.first()? {
        crate::dxbc::Index::Imm(v) => Some(*v),
        _ => None,
    }).collect();
    let fits_float = |base: u16, bytes: u16, buffer: u16| bytes > 0 && buffers.iter().any(|(slot, size)| *slot == buffer as u32 && base as u32 + bytes as u32 <= size * 16);
    let fits_texture = |t: u16, n: u16, smp: u16| n == 1 && textures5.contains(&(t as u32)) && samplers5.contains(&(smp as u32));
    let words = |b: &[u8]| b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<u16>>();
    let (a3, a5) = (words(sm3), words(sm5));
    if a3.len() != a5.len() || a3.len() < 3 {
        return (Vec::new(), Vec::new());
    }
    let mut used = vec![false; a3.len()];
    let mut floats = Vec::new();
    let mut textures = Vec::new();
    for c in &ctab.constants {
        let found = (0..a3.len() - 2).find(|&o| {
            if used[o] {
                return false;
            }
            match c.set {
                RegisterSet::Float => a3[o] as u32 == c.register as u32 * 16 && a3[o + 1] as u32 == c.count as u32 * 16 && a3[o + 2] == 0 && fits_float(a5[o], a5[o + 1], a5[o + 2]),
                RegisterSet::Sampler => a3[o] == c.register && a3[o + 1] == 1 && a3[o + 2] == c.register && fits_texture(a5[o], a5[o + 1], a5[o + 2]),
                _ => false,
            }
        });
        let Some(o) = found else { continue };
        used[o] = true;
        match c.set {
            RegisterSet::Float => floats.push((c.name.clone(), a5[o + 2] as u32, a5[o] as u32, a5[o + 1] as u32)),
            _ => textures.push((c.register, a5[o] as u32)),
        }
    }
    (floats, textures)
}

/// The renderer's own pixel constants in the SM5 buffer it keeps them in
/// (slot 2), by register: found by matching what thousands of shaders read
/// there with what their SM3 twins declare (`MinZ_MaxZRatio` is not read).
const SM5_PIXEL_FIXED: &[(&str, u32)] = &[("DiffuseOverrideParameter", 3), ("SpecularOverrideParameter", 4), ("GTimeVariablesPS", 9)];

/// An SM5 stage's float constants in its flat array: the paired ones, then
/// the renderer's, which no record carries: in a vertex shader the view
/// (`ViewProjectionMatrix`, `CameraPositionVS`…) sits in a buffer that
/// mirrors the SM3 registers (where a paired one sits at its register);
/// pixel shaders' are in [`SM5_PIXEL_FIXED`].
fn sm5_constants(paired: &[PairedFloat], sh3: &Shader, sh5: &crate::dxbc::Shader) -> Vec<FloatConstant> {
    let layout = crate::dxbc::Layout::of(sh5);
    let size = |slot: u32| sh5.constant_buffers().iter().find(|b| b.0 == slot).map(|b| b.1).unwrap_or(0);
    let register = |name: &str| sh3.constants.iter().find(|c| c.set == RegisterSet::Float && c.name.eq_ignore_ascii_case(name));
    let mut out: Vec<FloatConstant> = paired
        .iter()
        .filter_map(|(name, buffer, offset, bytes)| {
            let flat = layout.base(*buffer)? * 16 + offset;
            Some(FloatConstant { name: name.clone(), register: (flat / 16) as u16, count: bytes.div_ceil(16) as u16, offset: flat, bytes: *bytes })
        })
        .collect();
    let place = |out: &mut Vec<FloatConstant>, name: &str, slot: u32, reg: u32, count: u32| {
        let Some(base) = layout.base(slot) else { return };
        if reg + count <= size(slot) && !out.iter().any(|o| o.name.eq_ignore_ascii_case(name)) {
            let flat = (base + reg) * 16;
            out.push(FloatConstant { name: name.to_string(), register: (flat / 16) as u16, count: count as u16, offset: flat, bytes: count * 16 });
        }
    };
    if sh5.kind == crate::dxbc::Kind::Vertex {
        // Buffers past the first whose paired constants all sit at their SM3 register (or that none uses) mirror them.
        for &(slot, _) in layout.buffers.iter().filter(|b| b.0 != 0) {
            let mine: Vec<&PairedFloat> = paired.iter().filter(|p| p.1 == slot).collect();
            if mine.iter().all(|p| register(&p.0).is_some_and(|c| c.register as u32 * 16 == p.2)) {
                for c in sh3.constants.iter().filter(|c| c.set == RegisterSet::Float) {
                    place(&mut out, &c.name, slot, c.register as u32, c.count as u32);
                }
            }
        }
        // The determinant's sign: SM3 shaders often optimise it away (nothing to pair it with), SM5 keep it,
        // declared just before `WorldToLocal` (`FLocalVertexFactoryShaderParameters`).
        if !out.iter().any(|o| o.name.eq_ignore_ascii_case("LocalToWorldRotDeterminantFlip")) {
            if let Some(w) = out.iter().find(|o| o.name.eq_ignore_ascii_case("WorldToLocal")).map(|w| w.offset).filter(|o| *o >= 16) {
                let reg = w / 16 - 1;
                if !out.iter().any(|o| o.offset / 16 <= reg && reg < (o.offset + o.bytes).div_ceil(16)) {
                    out.push(FloatConstant { name: "LocalToWorldRotDeterminantFlip".into(), register: reg as u16, count: 1, offset: reg * 16, bytes: 4 });
                }
            }
        }
    } else {
        for (name, reg) in SM5_PIXEL_FIXED {
            if let Some(c) = register(name) {
                place(&mut out, name, 2, *reg, c.count as u32);
            }
        }
    }
    out
}

/// `PixelTexture2D_3` → 3.
fn suffix(name: &str, prefix: &str) -> Option<usize> {
    name.strip_prefix(prefix)?.parse().ok()
}

/// The game shader of `material` (an export of `pkg_name`, `info` its [`crate::material_ops::material_info`]).
/// `sm5`: the shader model 5 storages, to draw with the SM5 shaders (named through the SM3 ones) instead.
pub fn game_shader(cache: &mut PackageCache, storages: &[CodeStorage], sm5: Option<&[CodeStorage]>, pkg_name: &str, info: &MaterialInfo) -> Result<GameShader, String> {
    let res = own_resource(cache, info).ok_or("the material has no compiled resource")?;
    // The object's package first, then every package of its chain.
    let mut packages = vec![pkg_name.to_string()];
    for (p, _, _) in &info.chain {
        if !packages.iter().any(|q| q.eq_ignore_ascii_case(p)) {
            packages.push(p.clone());
        }
    }
    let (map, caches) = packages.iter().find_map(|p| find_map(&cache.get(p)?.pkg, &res)).ok_or("no package of the material's chain has its compiled shaders (only cooked, seek-free packages keep them)")?;
    let other_factories: Vec<String> = map.mesh_maps.iter().map(|m| m.vertex_factory.clone()).filter(|v| !v.eq_ignore_ascii_case(LOCAL_VERTEX_FACTORY)).collect();
    let pick = POLICIES.iter().find_map(|p| {
        let vs = format!("TBasePassVertexShader{p}FNoDensityPolicy");
        let vs_id = map.mesh_shader(LOCAL_VERTEX_FACTORY, &vs)?;
        let (ps, ps_id) = [format!("TBasePassPixelShader{p}SkyLight"), format!("TBasePassPixelShader{p}NoSkyLight")].into_iter().find_map(|t| map.mesh_shader(LOCAL_VERTEX_FACTORY, &t).map(|id| (t, id)))?;
        Some((vs, vs_id, ps, ps_id))
    });
    let Some((vertex_type, vs_id, pixel_type, ps_id)) = pick else {
        return Err(if other_factories.is_empty() { "the material has no base pass shaders for meshes".into() } else { format!("the material is only compiled for {} (meshes need {LOCAL_VERTEX_FACTORY})", other_factories.join(", ")) });
    };
    let code = |id: &Guid| -> Result<Vec<u8>, String> {
        let c = storages.iter().find_map(|s| s.code(id).ok().flatten()).ok_or_else(|| format!("shader {id} is not in the folder's shader code storage"))?;
        // Each blob starts with 4 bytes before the version token.
        c.get(4..).map(<[u8]>::to_vec).ok_or_else(|| format!("shader {id} is empty"))
    };
    let vs = Shader::parse(&code(&vs_id)?)?;
    let ps = Shader::parse(&code(&ps_id)?)?;
    if vs.stage != Stage::Vertex || ps.stage != Stage::Pixel {
        return Err("the shader map's pair is not a vertex and a pixel shader".into());
    }
    // Boolean constants (`bReceiveDynamicShadows`, `bUseExponentialHeightFog`…): the preview has none of those features.
    let mut module = d3d9::translate(&vs, &ps, &|_| false, &|_| false)?;
    let (mut vertex_constants, mut pixel_constants) = (float_constants(&vs), float_constants(&ps));
    // SM5: the same types of the same material's SM5 map, bound through the SM3 records.
    let mut sm5_textures: Option<(Vec<PairedTexture>, Vec<PairedTexture>)> = None;
    let mut fallback = None;
    // SM5, named through the SM3 pair; a material without (or not matching) falls back to SM3.
    let sm5_result = sm5.map(|st5| -> Result<_, String> {
        let record = |platform: u8, id: &Guid| caches.iter().filter(|c| c.platform == platform).flat_map(|c| c.shaders.iter()).find(|s| s.id == *id).cloned();
        let map5 = caches.iter().filter(|c| c.platform == 4).flat_map(|c| c.material_maps.iter()).find(|m| m.key == map.key).ok_or("the package has no shader model 5 shaders for this material")?;
        let (vs5_id, ps5_id) = (map5.mesh_shader(LOCAL_VERTEX_FACTORY, &vertex_type), map5.mesh_shader(LOCAL_VERTEX_FACTORY, &pixel_type));
        let (Some(vs5_id), Some(ps5_id)) = (vs5_id, ps5_id) else { return Err(format!("the SM5 map has no {vertex_type} / {pixel_type}")) };
        let code5 = |id: &Guid| -> Result<crate::dxbc::Shader, String> {
            let c = st5.iter().find_map(|s| s.code(id).ok().flatten()).ok_or_else(|| format!("shader {id} is not in the SM5 code storage"))?;
            crate::dxbc::Shader::parse(c.get(4..).unwrap_or_default())
        };
        let (vs5, ps5) = (code5(&vs5_id)?, code5(&ps5_id)?);
        let (Some(v3r), Some(v5r), Some(p3r), Some(p5r)) = (record(0, &vs_id), record(4, &vs5_id), record(0, &ps_id), record(4, &ps5_id)) else { return Err("a shader record is missing from the caches".into()) };
        let (vf, vt) = pair_parameters(&v3r.parameters, &v5r.parameters, &vs, &vs5);

        let (pf, pt) = pair_parameters(&p3r.parameters, &p5r.parameters, &ps, &ps5);
        if vf.is_empty() && pf.is_empty() {
            return Err("the SM5 parameters could not be matched with the SM3 ones".into());
        }
        let vertex_constants = sm5_constants(&vf, &vs, &vs5);
        // The per-draw registers (the placement): read from their own uniform.
        let mut draw = std::collections::BTreeMap::new();
        let mut draw_constants = Vec::new();
        let mut slot = 0u32;
        for name in d3d9::PER_DRAW {
            if let Some(c) = vertex_constants.iter().find(|c| c.name.eq_ignore_ascii_case(name)) {
                for k in 0..c.count as u32 {
                    draw.insert(c.register as u32 + k, slot + k);
                }
                draw_constants.push((c.name.clone(), slot as u16, c.count));
                slot += c.count as u32;
            }
        }
        let mut m5 = crate::dxbc::translate(&vs5, &ps5, &draw, slot as usize)?;
        m5.draw_constants = draw_constants;
        Ok((m5, sm5_constants(&vf, &vs, &vs5), sm5_constants(&pf, &ps, &ps5), (vt, pt)))
    });
    match sm5_result {
        Some(Ok((m5, vc, pc, tex))) => {
            module = m5;
            (vertex_constants, pixel_constants) = (vc, pc);
            sm5_textures = Some(tex);
        }
        Some(Err(e)) => fallback = Some(e),
        None => {}
    }

    // What each sampler takes: a material texture by its uniform expression, else the renderer's.
    let resolve = |cache: &mut PackageCache, expr: Option<&UniformExpr>| -> SamplerSource {
        let Some((param, index)) = expr.and_then(UniformExpr::texture) else { return SamplerSource::Missing("no such texture expression".into()) };
        if let Some(t) = param.and_then(|p| info.textures.iter().find(|t| t.parameter.eq_ignore_ascii_case(p))) {
            return SamplerSource::Texture(t.package.clone(), t.export);
        }
        let Some(&t) = usize::try_from(index).ok().and_then(|i| res.textures.get(i)) else { return SamplerSource::Missing(format!("texture {index} of the material")) };
        match cache.resolve(&res.package, t) {
            Some((p, e)) => SamplerSource::Texture(p, e),
            None => SamplerSource::Missing(cache.describe(&res.package, t)),
        }
    };
    // SM5 texture slot → the SM3 sampler register whose name it has.
    let sm3_register = |register: u16, vertex: bool| -> Option<u16> {
        match &sm5_textures {
            None => Some(register),
            Some((vt, pt)) => (if vertex { vt } else { pt }).iter().find(|(_, t)| *t == register as u32).map(|(s, _)| *s),
        }
    };
    let samplers_of = |cache: &mut PackageCache, sh: &Shader, list: &[(u16, SamplerKind)], textures: &[UniformExpr], prefix: &str| -> Vec<Sampler> {
        let vertex = prefix.starts_with("Vertex");
        list.iter()
            .map(|&(register, kind)| {
                let name = sm3_register(register, vertex).and_then(|r| sh.constants.iter().find(|c| c.set == RegisterSet::Sampler && c.register == r)).map(|c| c.name.clone()).unwrap_or_default();
                let cube_prefix = prefix.replace("2D", "Cube");
                let source = if kind == SamplerKind::Cube {
                    // A cube map of the material (its six faces are sent with it), else the renderer's.
                    match suffix(&name, &cube_prefix) {
                        Some(k) => match resolve(cache, map.uniforms.cube_textures.get(k)) {
                            SamplerSource::Texture(p, e) => SamplerSource::Texture(p, e),
                            _ => SamplerSource::Cube,
                        },
                        None => SamplerSource::Cube,
                    }
                } else if let Some(k) = suffix(&name, prefix) {
                    resolve(cache, textures.get(k))
                } else {
                    SamplerSource::Renderer
                };
                let cube_k = suffix(&name, &cube_prefix);
                let expr = match (suffix(&name, prefix), cube_k) {
                    (Some(k), _) => textures.get(k),
                    (_, Some(k)) => map.uniforms.cube_textures.get(k),
                    _ => None,
                };
                let expression = expr.and_then(UniformExpr::texture).map(|(p, i)| p.map(str::to_string).unwrap_or_else(|| format!("#{i}")));
                Sampler { register, name, kind, source, expression }
            })
            .collect()
    };
    let samplers = samplers_of(cache, &ps, &module.samplers, &map.uniforms.pixel.textures_2d, "PixelTexture2D_");
    let vertex_samplers = samplers_of(cache, &vs, &module.vertex_samplers, &map.uniforms.vertex.textures_2d, "VertexTexture2D_");
    Ok(GameShader {
        friendly_name: map.friendly_name.clone(),
        vertex_factory: LOCAL_VERTEX_FACTORY.into(),
        vertex_type,
        pixel_type,
        vertex_constants,
        pixel_constants,
        pixel_vectors: map.uniforms.pixel.vectors.clone(),
        pixel_scalars: map.uniforms.pixel.scalars.clone(),
        vertex_vectors: map.uniforms.vertex.vectors.clone(),
        vertex_scalars: map.uniforms.vertex.scalars.clone(),
        platform: if sm5_textures.is_some() { "sm5" } else { "sm3" },
        module,
        samplers,
        vertex_samplers,
        other_factories,
        fallback,
    })
}

/// The six faces of a `TextureCube` (`FacePosX`, `FaceNegX`, `FacePosY`,
/// `FaceNegY`, `FacePosZ`, `FaceNegZ`: Direct3D's and wgpu's layer order),
/// as (package, export); `None` when it is not a cube or a face is missing.
pub fn cube_faces(cache: &mut PackageCache, pkg_name: &str, export: usize) -> Option<Vec<(String, usize)>> {
    let obj = cache.object(pkg_name, export)?;
    let refs: Vec<PackageIndex> = {
        let lp = cache.get(pkg_name)?;
        ["FacePosX", "FaceNegX", "FacePosY", "FaceNegY", "FacePosZ", "FaceNegZ"]
            .iter()
            .map(|f| match obj.property(f, &lp.pkg).map(|p| &p.value) {
                Some(Value::Object(i)) if !i.is_null() => Some(*i),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?
    };
    refs.into_iter().map(|r| cache.resolve(pkg_name, r)).collect()
}

/// Whether a texture holds colour (sampled with the sRGB curve undone) or data (normals, masks): its `SRGB` property.
pub fn texture_is_srgb(cache: &mut PackageCache, pkg_name: &str, export: usize) -> bool {
    let Some(obj) = cache.object(pkg_name, export) else { return true };
    let Some(lp) = cache.get(pkg_name) else { return true };
    !matches!(obj.property("SRGB", &lp.pkg).map(|p| &p.value), Some(Value::Bool(false)))
}

/// The folder's shader code storages for the PC Direct3D 9 renderer.
pub fn storages(cooked_dir: &Path) -> Vec<CodeStorage> {
    crate::shadercache::open_storages(cooked_dir, "PC-D3D-SM3")
}

/// The folder's shader code storages for the Direct3D 11 (shader model 5) renderer.
pub fn storages_sm5(cooked_dir: &Path) -> Vec<CodeStorage> {
    crate::shadercache::open_storages(cooked_dir, "PC-D3D-SM5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_names_give_their_expression() {
        assert_eq!(suffix("PixelTexture2D_3", "PixelTexture2D_"), Some(3));
        assert_eq!(suffix("PixelTextureCube_0", "PixelTexture2D_"), None);
        assert_eq!(suffix("LightAttenuationTexture", "PixelTexture2D_"), None);
    }
}
