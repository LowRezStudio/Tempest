//! Static light maps: reading a component's light map back to linear light
//! (`info`, `decode`) and baking new ones (`scene` → `bake` → `write`).
//! See `encode.rs` for the format and `bake.rs` for what is ported from
//! Lightmass and what is approximated.

pub mod bake;
pub mod bvh;
pub mod encode;
pub mod scene;
pub mod write;

use serde_json::{json, Value as J};
use tempest_upk::io::{Error, Guid, PackageIndex, Result};

use crate::dds::{decode_rgba, PixelFormat};
use crate::edit::Editor;
use crate::level::LightMap;
use crate::object::Native;
use crate::texcache::TextureSource;
use crate::texture_ops::{available_mips, format_name};
use encode::Decoded;

pub fn guid_text(g: &Guid) -> String {
    format!("{:08X}{:08X}{:08X}{:08X}", g.a, g.b, g.c, g.d)
}

/// The light map of LOD `lod` of a static mesh component.
pub fn component_light_map(ed: &mut Editor, i: usize, lod: usize) -> Result<LightMap> {
    ed.with_object(i, |_, o| match &o.native {
        Native::StaticMeshComponent(c) => Ok(c.lod_data.get(lod).map(|l| l.light_map.clone()).unwrap_or_default()),
        _ => Err(Error::Format("not a static mesh component".into())),
    })?
}

/// A decoded light map: the component's texels (padding included).
pub struct DecodedMap {
    pub width: usize,
    pub height: usize,
    /// Rectangle in the atlas (x, y) and the atlas size.
    pub origin: (usize, usize),
    pub atlas: (usize, usize),
    pub texels: Vec<Decoded>,
    /// Raw bytes of the two textures over the rectangle (DXT1 decoded).
    pub bytes: Vec<([u8; 3], [u8; 3])>,
    pub scales: [[f32; 3]; 3],
}

fn texture_rgba(ed: &mut Editor, src: &TextureSource, t: usize) -> Result<(usize, usize, Vec<u8>)> {
    let db = ed.db;
    let name = ed.name.clone();
    ed.with_object(t, |pkg, o| -> Result<(usize, usize, Vec<u8>)> {
        let fname = format_name(db, pkg, o);
        let format = PixelFormat::from_name(&fname).ok_or_else(|| Error::Format(format!("unsupported pixel format {fname}")))?;
        let mips = available_mips(pkg, &name, src, t, o)?;
        let (w, h, d) = mips.first().ok_or_else(|| Error::Format("no mips".into()))?;
        // The top mip may be stripped from the cache: only a full-size one decodes the rectangle.
        let full = match &o.native {
            Native::Texture2D(tx) => tx.mips.first().map(|m| m.size_x as u32 == *w).unwrap_or(true),
            _ => true,
        };
        if !full {
            return Err(Error::Format(format!("the top mip of {} is not available", pkg.object_name(PackageIndex::export(t)))));
        }
        Ok((*w as usize, *h as usize, decode_rgba(format, *w, *h, d)?))
    })?
}

/// Decode a component's light map from its textures.
pub fn decode(ed: &mut Editor, src: &TextureSource, i: usize, lod: usize) -> Result<DecodedMap> {
    let LightMap::TwoD { textures, coordinate_scale, coordinate_bias, .. } = component_light_map(ed, i, lod)? else {
        return Err(Error::Format("this component has no texture light map".into()));
    };
    let mut scales = [[0f32; 3]; 3];
    for (k, t) in textures.iter().enumerate() {
        scales[k] = t.1;
    }
    let nac = textures[0].0.export_index().ok_or_else(|| Error::Format("light map texture 0 is not in this package".into()))?;
    let dmc = textures[1].0.export_index().ok_or_else(|| Error::Format("light map texture 1 is not in this package".into()))?;
    let (w, h, a) = texture_rgba(ed, src, nac)?;
    let (w2, h2, b) = texture_rgba(ed, src, dmc)?;
    if (w, h) != (w2, h2) {
        return Err(Error::Format("the two light map textures differ in size".into()));
    }
    // Inner rectangle from the scale/bias, one padding texel around it.
    let ix = (coordinate_bias[0] * w as f32).round() as i64;
    let iy = (coordinate_bias[1] * h as f32).round() as i64;
    let iw = (coordinate_scale[0] * w as f32).round() as i64;
    let ih = (coordinate_scale[1] * h as f32).round() as i64;
    let (x0, y0) = ((ix - 1).max(0) as usize, (iy - 1).max(0) as usize);
    let (x1, y1) = (((ix + iw + 1) as usize).min(w), ((iy + ih + 1) as usize).min(h));
    let mut texels = Vec::new();
    let mut bytes = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let k = (y * w + x) * 4;
            let n = [a[k], a[k + 1], a[k + 2]];
            let d = [b[k], b[k + 1], b[k + 2]];
            texels.push(Decoded::from_bytes(n, d, &scales));
            bytes.push((n, d));
        }
    }
    Ok(DecodedMap { width: x1 - x0, height: y1 - y0, origin: (x0, y0), atlas: (w, h), texels, bytes, scales })
}

/// `light.info` / `upk light info`: the light map of a component as stored.
pub fn info_json(ed: &mut Editor, i: usize) -> Result<J> {
    let lods = ed.with_object(i, |_, o| match &o.native {
        Native::StaticMeshComponent(c) => c.lod_data.iter().map(|l| l.light_map.clone()).collect::<Vec<_>>(),
        _ => Vec::new(),
    })?;
    let pkg = &ed.pkg;
    let lods: Vec<J> = lods
        .iter()
        .enumerate()
        .map(|(lod, lm)| match lm {
            LightMap::None => json!({ "lod": lod, "kind": "none" }),
            LightMap::OneD { light_guids, scale_vectors, .. } => json!({ "lod": lod, "kind": "vertex", "lights": light_guids.iter().map(guid_text).collect::<Vec<_>>(), "scales": scale_vectors }),
            LightMap::TwoD { light_guids, textures, coordinate_scale, coordinate_bias } => json!({
                "lod": lod,
                "kind": "texture",
                "lights": light_guids.iter().map(guid_text).collect::<Vec<_>>(),
                "textures": textures.iter().map(|(t, s)| json!({ "texture": (!t.is_null()).then(|| pkg.object_path(*t)), "index": t.export_index(), "scale": s })).collect::<Vec<_>>(),
                "coordinateScale": coordinate_scale,
                "coordinateBias": coordinate_bias,
            }),
        })
        .collect();
    Ok(json!({ "export": i, "name": ed.pkg.object_name(PackageIndex::export(i)), "lods": lods }))
}

/// Irradiance of a decoded map as an RGBA8 image (`exposure` scales the
/// linear light before the 1/2.2 gamma; 1 shows 1.0 as white).
pub fn irradiance_rgba(m: &DecodedMap, exposure: f32) -> Vec<u8> {
    let mut out = Vec::with_capacity(m.texels.len() * 4);
    for t in &m.texels {
        let e = t.irradiance();
        for c in e {
            out.push(encode::quantize_value((c * exposure).min(1.0)));
        }
        out.push(255);
    }
    out
}

/// Compare a shipped light map with a bake of the same component (both
/// padded `size`×`size`): irradiance along the vertex normal over the texels
/// the bake maps, luminance-weighted. `None` when the sizes differ.
pub fn compare(shipped: &DecodedMap, ours: &[encode::Sample], size: usize) -> Option<J> {
    if shipped.width != size || shipped.height != size || ours.len() != size * size {
        return None;
    }
    let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (mut n, mut sa, mut sb, mut abs, mut max) = (0usize, 0f64, 0f64, 0f64, 0f64);
    let mut ratios = Vec::new();
    for (s, d) in ours.iter().zip(&shipped.texels) {
        if !s.mapped {
            continue;
        }
        let a = lum(Decoded::of_sample(s).irradiance()) as f64;
        let b = lum(d.irradiance()) as f64;
        n += 1;
        sa += a;
        sb += b;
        abs += (a - b).abs();
        max = max.max((a - b).abs());
        if b > 0.02 {
            ratios.push(a / b);
        }
    }
    if n == 0 {
        return None;
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = ratios.get(ratios.len() / 2).copied();
    Some(json!({ "texels": n, "meanBaked": sa / n as f64, "meanShipped": sb / n as f64, "meanAbsDiff": abs / n as f64, "maxAbsDiff": max, "medianRatio": median }))
}

// ----------------------------------------------------------------------------
// A whole bake
// ----------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use scene::{Instance, LevelData, MeshData, MeshKey};

/// Per level: the allocations to write.
pub struct Output {
    pub per_level: Vec<Vec<write::Allocation>>,
    /// Per baked component: level, export, size, mapped texels, lights held.
    pub report: Vec<J>,
    pub skipped: Vec<J>,
    pub triangles: usize,
    pub lights: usize,
}

fn bounds(inst: &Instance, mesh: &MeshData) -> ([f32; 3], f32) {
    let m = &inst.world;
    let pts: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .map(|p| {
            let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
            [(x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0]) as f32, (x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1]) as f32, (x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2]) as f32]
        })
        .collect();
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &pts {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let r = pts.iter().map(|p| bvh::length(bvh::sub(*p, c))).fold(0f32, f32::max);
    (c, r)
}

/// Bake every instance `select` picks, against the lights and shadow
/// casters of all `levels`. `progress(done, total)` is called between
/// components.
pub fn run(levels: &[LevelData], meshes: &HashMap<MeshKey, Arc<MeshData>>, select: &dyn Fn(&Instance) -> bool, opts: &bake::Options, progress: &mut dyn FnMut(usize, usize)) -> Output {
    let lights: Vec<scene::Light> = levels.iter().flat_map(|l| l.lights.iter().cloned()).collect();
    let world = levels.iter().find_map(|l| l.world.clone()).unwrap_or(scene::WorldSettings { diffuse_boost: 5.0, ..Default::default() });
    let casters: Vec<(&Instance, Arc<MeshData>)> = levels.iter().flat_map(|l| l.instances.iter()).filter(|i| i.static_lighting).filter_map(|i| meshes.get(&i.mesh).map(|m| (i, m.clone()))).collect();
    let sc = bake::Scene::build(lights, &casters, world);
    let mut out = Output { per_level: levels.iter().map(|_| Vec::new()).collect(), report: Vec::new(), skipped: Vec::new(), triangles: sc.bvh.len(), lights: sc.lights.len() };
    let targets: Vec<&Instance> = levels.iter().flat_map(|l| l.instances.iter()).filter(|i| select(i)).collect();
    let total = targets.len();
    let rows = AtomicUsize::new(0);
    for (k, inst) in targets.into_iter().enumerate() {
        progress(k, total);
        let skip = |why: &str| json!({ "level": inst.level, "export": inst.export, "name": inst.name, "reason": why });
        let Some(mesh) = meshes.get(&inst.mesh) else {
            out.skipped.push(skip("mesh not found"));
            continue;
        };
        if !inst.static_lighting {
            out.skipped.push(skip("not statically lit (movable owner or bAcceptsLights off)"));
            continue;
        }
        let Some(size) = bake::resolution(inst, mesh) else {
            out.skipped.push(skip("no light map UVs or resolution (vertex light maps are not baked)"));
            continue;
        };
        let target = bake::Target { instance: inst, mesh: mesh.clone(), size };
        let baked = bake::bake(&sc, &target, opts, &rows);
        // Every light that reaches the component is listed, lit or shadowed:
        // a static light missing from the list would light it dynamically.
        let (c, r) = bounds(inst, mesh);
        let held: Vec<&scene::Light> = sc
            .lights
            .iter()
            .filter(|l| scene::overlaps(&l.channels, &inst.channels))
            .filter(|l| scene::affects_bounds(l, c, r))
            .collect();
        let guids: Vec<_> = held.iter().map(|l| l.lightmap_guid).collect();
        let lit: Vec<String> = baked.lights.iter().map(|&i| sc.lights[i].name.clone()).collect();
        let lower: Vec<(usize, &Arc<MeshData>)> = mesh.lower.iter().enumerate().filter(|(_, m)| m.has_lightmap_uvs).map(|(l, m)| (l + 1, m)).collect();
        out.report.push(json!({ "level": inst.level, "export": inst.export, "name": inst.name, "size": size, "lods": 1 + lower.len(), "mapped": baked.mapped, "lights": held.len(), "lit": lit }));
        out.per_level[inst.level].push(write::Allocation { component: inst.export, lod: 0, size, samples: baked.samples, light_guids: guids.clone() });
        // Lower LODs: light maps of their own, of the same size (shipped
        // components hold one allocation per LOD, in the same atlas).
        for (lod, m) in lower {
            let baked = bake::bake(&sc, &bake::Target { instance: inst, mesh: m.clone(), size }, opts, &rows);
            out.per_level[inst.level].push(write::Allocation { component: inst.export, lod, size, samples: baked.samples, light_guids: guids.clone() });
        }
    }
    progress(total, total);
    out
}
