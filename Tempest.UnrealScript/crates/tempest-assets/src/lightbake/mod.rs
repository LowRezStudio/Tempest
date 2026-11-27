//! Static light maps: reading a component's light map back to linear light
//! (`info`, `decode`) and baking new ones (`scene` → `bake` → `write`).
//! See `encode.rs` for the format and `bake.rs` for what is ported from
//! Lightmass and what is approximated.

pub mod bake;
pub mod bvh;
pub mod encode;
pub mod scene;
pub mod trace;
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
    // Shaded vs lit: the texels below / above the shipped map's median
    // (each side's summed light, baked over shipped); and blotchiness: the
    // mean |texel − its mapped neighbours' mean|, over the mean.
    let lum_at = |k: usize, baked: bool| -> Option<f64> {
        let s = ours.get(k)?;
        if !s.mapped {
            return None;
        }
        Some(if baked { lum(Decoded::of_sample(s).irradiance()) } else { lum(shipped.texels[k].irradiance()) } as f64)
    };
    let mut shipped_l: Vec<f64> = (0..ours.len()).filter_map(|k| lum_at(k, false)).collect();
    shipped_l.sort_by(|a, b| a.total_cmp(b));
    let cut = shipped_l.get(shipped_l.len() / 2).copied().unwrap_or(0.0);
    let (mut dark, mut lit) = ([0f64; 2], [0f64; 2]);
    // The colour of the shaded texels: summed RGB, baked then shipped.
    let mut shade_rgb = [[0f64; 3]; 2];
    for (k, sample) in ours.iter().enumerate() {
        let (Some(b), Some(a)) = (lum_at(k, false), lum_at(k, true)) else { continue };
        let side = if b < cut { &mut dark } else { &mut lit };
        side[0] += a;
        side[1] += b;
        if b < cut {
            let (cb, cs) = (Decoded::of_sample(sample).irradiance(), shipped.texels[k].irradiance());
            for ch in 0..3 {
                shade_rgb[0][ch] += cb[ch] as f64;
                shade_rgb[1][ch] += cs[ch] as f64;
            }
        }
    }
    let chroma = |c: [f64; 3]| {
        let s = c[0] + c[1] + c[2];
        (s > 0.0).then(|| [c[0] / s, c[1] / s, c[2] / s])
    };
    let noise = |baked: bool| -> Option<f64> {
        let (mut d, mut m, mut c) = (0f64, 0f64, 0usize);
        for y in 1..size.saturating_sub(1) {
            for x in 1..size - 1 {
                let Some(v) = lum_at(y * size + x, baked) else { continue };
                let nb: Vec<f64> = [(0, 1), (2, 1), (1, 0), (1, 2)].iter().filter_map(|&(dx, dy)| lum_at((y + dy - 1) * size + x + dx - 1, baked)).collect();
                if nb.len() < 4 {
                    continue;
                }
                d += (v - nb.iter().sum::<f64>() / 4.0).abs();
                m += v;
                c += 1;
            }
        }
        (c > 0 && m > 0.0).then(|| d / m)
    };
    Some(json!({ "texels": n, "meanBaked": sa / n as f64, "meanShipped": sb / n as f64, "meanAbsDiff": abs / n as f64, "maxAbsDiff": max, "medianRatio": median, "shadedRatio": (dark[1] > 0.0).then(|| dark[0] / dark[1]), "litRatio": (lit[1] > 0.0).then(|| lit[0] / lit[1]), "noiseBaked": noise(true), "noiseShipped": noise(false), "shadeChromaBaked": chroma(shade_rgb[0]), "shadeChromaShipped": chroma(shade_rgb[1]) }))
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
    /// Path-traced mode: what traced the rays, why not a GPU (when one was
    /// offered and could not be used), the scale the albedos got.
    pub tracer: Option<String>,
    pub tracer_note: Option<String>,
    pub albedo_scale: Option<f32>,
}

/// What the path-traced mode uses beyond the levels.
#[derive(Default)]
pub struct Extras<'a> {
    /// Material colours, and which one each element of a caster has: by
    /// (level, component export, element).
    pub albedos: Vec<Arc<bake::Albedo>>,
    pub albedo_of: HashMap<(usize, usize, usize), u32>,
    /// Makes a GPU tracer (else the CPU traces).
    pub tracer: Option<&'a trace::TracerFactory>,
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
pub fn run(levels: &[LevelData], meshes: &HashMap<MeshKey, Arc<MeshData>>, select: &dyn Fn(&Instance) -> bool, opts: &bake::Options, extras: Extras, progress: &mut dyn FnMut(usize, usize)) -> Output {
    let lights: Vec<scene::Light> = levels.iter().flat_map(|l| l.lights.iter().cloned()).collect();
    let world = levels.iter().find_map(|l| l.world.clone()).unwrap_or(scene::WorldSettings { diffuse_boost: 5.0, ..Default::default() });
    let casters: Vec<(&Instance, Arc<MeshData>)> = levels.iter().flat_map(|l| l.instances.iter()).filter(|i| i.static_lighting).filter_map(|i| meshes.get(&i.mesh).map(|m| (i, m.clone()))).collect();
    let albedos = if opts.path_traced { extras.albedos } else { Vec::new() };
    let albedo_of = |i: &Instance, _: &MeshData, e: usize| extras.albedo_of.get(&(i.level, i.export, e)).copied();
    let mut sc = bake::Scene::build(lights, &casters, world, albedos, &albedo_of);
    let mut out = Output { per_level: levels.iter().map(|_| Vec::new()).collect(), report: Vec::new(), skipped: Vec::new(), triangles: sc.bvh.len(), lights: sc.lights.len(), tracer: None, tracer_note: None, albedo_scale: None };
    // Path-traced: the albedos scaled to the fitted mean, a GPU tracer if one can be made.
    let gpu: Option<Box<dyn trace::Tracer>> = match (opts.path_traced, extras.tracer) {
        (true, Some(make)) => match make(&sc.bvh) {
            Ok(t) => Some(t),
            Err(e) => {
                out.tracer_note = Some(e);
                None
            }
        },
        _ => None,
    };
    if opts.path_traced {
        if let Some(m) = sc.albedo_mean {
            sc.albedo_scale = opts.albedo / m;
            out.albedo_scale = Some(sc.albedo_scale);
        }
    }
    let cpu = trace::CpuTracer(&sc.bvh);
    let tracer: &dyn trace::Tracer = match &gpu {
        Some(g) => g.as_ref(),
        None => &cpu,
    };
    if opts.path_traced {
        out.tracer = Some(tracer.name());
    }
    let bake_one = |target: &bake::Target, rows: &AtomicUsize| if opts.path_traced { bake::bake_traced(&sc, target, opts, tracer, rows) } else { bake::bake(&sc, target, opts, rows) };
    let targets: Vec<&Instance> = levels.iter().flat_map(|l| l.instances.iter()).filter(|i| select(i)).collect();
    let rows = AtomicUsize::new(0);
    // What to bake: each component's LOD 0 and lower LODs with light map UVs
    // (shipped components hold one allocation per LOD, in the same atlas).
    let mut jobs: Vec<(&Instance, Arc<MeshData>, usize)> = Vec::new();
    let mut pieces: Vec<(usize, usize, Arc<MeshData>)> = Vec::new();
    for inst in targets {
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
        pieces.push((jobs.len(), 0, mesh.clone()));
        for (l, m) in mesh.lower.iter().enumerate().filter(|(_, m)| m.has_lightmap_uvs) {
            pieces.push((jobs.len(), l + 1, m.clone()));
        }
        jobs.push((inst, mesh.clone(), size));
    }
    let bake_piece = |(j, _, m): &(usize, usize, Arc<MeshData>)| bake_one(&bake::Target { instance: jobs[*j].0, mesh: m.clone(), size: jobs[*j].2 }, &rows);
    let baked: Vec<bake::Baked> = if opts.path_traced {
        // Components side by side: one shades while another's rays are traced.
        let done = AtomicUsize::new(0);
        std::thread::scope(|s| {
            let work = s.spawn(|| {
                tempest_unrealscript::par::map(&pieces, |p| {
                    let b = bake_piece(p);
                    if p.1 == 0 {
                        done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    b
                })
            });
            while !work.is_finished() {
                progress(done.load(std::sync::atomic::Ordering::Relaxed), jobs.len());
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            work.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
        })
    } else {
        pieces
            .iter()
            .map(|p| {
                if p.1 == 0 {
                    progress(p.0, jobs.len());
                }
                bake_piece(p)
            })
            .collect()
    };
    for (p, baked) in pieces.iter().zip(baked) {
        let (inst, mesh, size) = (jobs[p.0].0, &jobs[p.0].1, jobs[p.0].2);
        // Every light that reaches the component is listed, lit or shadowed:
        // a static light missing from the list would light it dynamically.
        let (c, r) = bounds(inst, mesh);
        let guids: Vec<_> = sc.lights.iter().filter(|l| scene::overlaps(&l.channels, &inst.channels)).filter(|l| scene::affects_bounds(l, c, r)).map(|l| l.lightmap_guid).collect();
        if p.1 == 0 {
            let lit: Vec<String> = baked.lights.iter().map(|&i| sc.lights[i].name.clone()).collect();
            let lods = 1 + mesh.lower.iter().filter(|m| m.has_lightmap_uvs).count();
            out.report.push(json!({ "level": inst.level, "export": inst.export, "name": inst.name, "size": size, "lods": lods, "mapped": baked.mapped, "lights": guids.len(), "lit": lit }));
        }
        out.per_level[inst.level].push(write::Allocation { component: inst.export, lod: p.1, size, samples: baked.samples, light_guids: guids });
    }
    progress(jobs.len(), jobs.len());
    out
}
