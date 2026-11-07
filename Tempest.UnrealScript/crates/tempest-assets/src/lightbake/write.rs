//! Storing baked light maps the way the engine's light map encoder does
//! (`FLightMap2D::AllocateLightMap`, `FLightMapPendingTexture`):
//!
//! * each component is an allocation of its padded `size`×`size` texels,
//!   quantized on its own (its own `ScaleVectors`), placed in an atlas on
//!   4-texel (DXT block) boundaries;
//! * an atlas is two `LightMapTexture2D`s, `NormalizedAverageColor<i>_<n>`
//!   and `DirectionalMaxComponent<i>_<n>` (`GetLightmapName`), DXT1 with
//!   the full mip chain; the simple light map (coefficient 2) is not
//!   written and its scale stays 0, as in every shipped map
//!   (`bShouldGenerateSimpleLightmaps` off);
//! * the component's `FLightMap2D`: the lights it holds (their
//!   `LightmapGuid`), the two textures with their scales, and
//!   `CoordinateScale = (size - 2) / atlas`, `CoordinateBias = (x + 1) / atlas`
//!   (the padding texel skipped, `StartEncoding`).
//!
//! New textures are copies of a light map texture the package already has
//! (same class, properties and flags), their mips stored inline with
//! `NeverStream` (`texture_ops::import`) and `LightmapFlags` 0 (not
//! streamed), so the package needs no texture file cache entry.

use tempest_upk::io::{Error, Guid, PackageIndex, Result};

use super::encode::{encode_dxt1, mip_chain, quantize, Quantized, Sample, Texel};
use crate::dds::PixelFormat;
use crate::edit::Editor;
use crate::object::Parsed;
use crate::visit::references;
use crate::level::{LightMap, StaticMeshComponentLod};
use crate::object::Native;
use crate::reflect::ClassDb;
use crate::texture_ops::{import, ImportOptions, ImportSource};

/// One component's result to store.
pub struct Allocation {
    pub component: usize,
    /// The component's LOD (`LODData` entry) it lights.
    pub lod: usize,
    pub size: usize,
    pub samples: Vec<Sample>,
    pub light_guids: Vec<Guid>,
}

/// What was written.
#[derive(Debug, Default)]
pub struct Written {
    pub textures: Vec<(usize, String)>,
    pub atlases: Vec<(usize, usize)>,
    pub components: usize,
    pub cleared_lods: usize,
    /// Light map / shadow map objects of the package that nothing uses any
    /// more (the rebaked components' old ones above all), deleted: the game
    /// loads every object of a map package, the unused ones too.
    pub removed: Vec<String>,
}

/// Shelf packing of square allocations into atlases of at most `max`.
fn pack(sizes: &[usize], max: usize) -> Vec<(usize, usize, usize)> {
    let align = |v: usize| v.div_ceil(4) * 4;
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut out = vec![(0, 0, 0); sizes.len()];
    // Per atlas: shelves (y, height, x).
    let mut atlases: Vec<Vec<(usize, usize, usize)>> = Vec::new();
    for i in order {
        let s = align(sizes[i]).min(max);
        let mut placed = false;
        for (a, shelves) in atlases.iter_mut().enumerate() {
            if let Some(sh) = shelves.iter_mut().find(|sh| sh.1 >= s && sh.2 + s <= max) {
                out[i] = (a, sh.2, sh.0);
                sh.2 += s;
                placed = true;
                break;
            }
            let top = shelves.last().map(|sh| sh.0 + sh.1).unwrap_or(0);
            if top + s <= max {
                shelves.push((top, s, s));
                out[i] = (a, 0, top);
                placed = true;
                break;
            }
        }
        if !placed {
            atlases.push(vec![(0, s, s)]);
            out[i] = (atlases.len() - 1, 0, 0);
        }
    }
    out
}

/// `GetLightmapName`: the first unused `<base><i>_<n>`.
fn lightmap_name(ed: &Editor, base: &str, n: usize) -> String {
    for i in 0.. {
        let name = format!("{base}{i}_{n}");
        if !(0..ed.pkg.exports.len()).any(|j| ed.pkg.exports[j].outer.is_null() && ed.pkg.object_name(PackageIndex::export(j)).eq_ignore_ascii_case(&name)) {
            return name.to_ascii_lowercase();
        }
    }
    unreachable!()
}

/// A light map texture of the package to copy new ones from.
pub fn template(ed: &Editor) -> Option<usize> {
    (0..ed.pkg.exports.len()).find(|&i| ed.pkg.class_name_of(PackageIndex::export(i)).eq_ignore_ascii_case("LightMapTexture2D"))
}

/// Give a level without light map textures one to model new ones on, copied
/// from another level of the map (it references nothing).
pub fn ensure_template(ed: &mut Editor, from: &mut Editor) -> Result<()> {
    if template(ed).is_some() {
        return Ok(());
    }
    let t = template(from).ok_or_else(|| Error::Format("no level of this map has a light map texture to model new ones on".into()))?;
    ed.copy_with(from, t, None, crate::edit::CopyMode::Clear)?;
    Ok(())
}

/// Write `allocs` into the package: atlases of at most `max_size` texels.
pub fn write(ed: &mut Editor, db: &ClassDb, allocs: &[Allocation], max_size: usize) -> Result<Written> {
    let tpl = template(ed).ok_or_else(|| Error::Format("this level has no light map texture to model new ones on".into()))?;
    let mut written = Written::default();
    let places = pack(&allocs.iter().map(|a| a.size).collect::<Vec<_>>(), max_size);
    let count = places.iter().map(|p| p.0 + 1).max().unwrap_or(0);
    // Quantize every allocation on its own.
    let quantized: Vec<([[f32; 3]; 3], Vec<Quantized>)> = allocs.iter().map(|a| quantize(&a.samples)).collect();
    let stamp = (0..ed.pkg.exports.len()).count();
    // Light and shadow maps nothing uses once the components point at the
    // new ones are deleted at the end: their old ones, and those copies made
    // by an older Studio left behind (which the game cannot load).
    let old: Vec<usize> = (0..ed.pkg.exports.len()).filter(|&i| ["LightMapTexture2D", "ShadowMapTexture2D", "ShadowMap2D", "ShadowMap1D"].iter().any(|c| db.class_is_a(&ed.pkg.class_name_of(PackageIndex::export(i)), c))).collect();
    for atlas in 0..count {
        let members: Vec<usize> = (0..allocs.len()).filter(|&i| places[i].0 == atlas).collect();
        let used_w = members.iter().map(|&i| places[i].1 + allocs[i].size).max().unwrap_or(4);
        let used_h = members.iter().map(|&i| places[i].2 + allocs[i].size).max().unwrap_or(4);
        let (w, h) = (used_w.next_power_of_two().max(4), used_h.next_power_of_two().max(4));
        let mut tex_index = [0usize; 2];
        for (coef, base) in ["NormalizedAverageColor", "DirectionalMaxComponent"].iter().enumerate() {
            let mut top: Vec<Texel> = vec![[0, 0, 0, 0]; w * h];
            for &i in &members {
                let (_, x0, y0) = places[i];
                let s = allocs[i].size;
                for y in 0..s {
                    for x in 0..s {
                        let q = quantized[i].1[y * s + x];
                        let c = q.c[coef];
                        top[(y0 + y) * w + x0 + x] = [c[0], c[1], c[2], q.coverage];
                    }
                }
            }
            let chain = mip_chain(w, h, top, 4);
            let mips: Vec<Vec<u8>> = chain.iter().map(|(mw, mh, d)| encode_dxt1(*mw, *mh, d)).collect();
            let name = lightmap_name(ed, base, stamp + atlas);
            let t = ed.duplicate(tpl)?;
            ed.rename(t, &name)?;
            let mut obj = ed.object(t)?.clone();
            import(db, &mut ed.pkg, t, &mut obj, ImportSource::Encoded { format: PixelFormat::Dxt1, width: w as u32, height: h as u32, mips }, &ImportOptions { mips: Some(true), ..Default::default() })?;
            if let Native::Texture2D(tx) = &mut obj.native {
                tx.lightmap_flags = Some(0);
            }
            ed.replace_object(t, obj)?;
            tex_index[coef] = t;
            written.textures.push((t, name));
        }
        written.atlases.push((w, h));
        for &i in &members {
            let a = &allocs[i];
            let (_, x0, y0) = places[i];
            let (scale, bias_off) = if a.size > 2 { ((a.size - 2) as f32, 1.0) } else { (a.size as f32, 0.0) };
            let scales = quantized[i].0;
            let lm = LightMap::TwoD {
                light_guids: a.light_guids.clone(),
                textures: [(PackageIndex::export(tex_index[0]), scales[0]), (PackageIndex::export(tex_index[1]), scales[1]), (PackageIndex::NONE, [0.0; 3])],
                coordinate_scale: [scale / w as f32, scale / h as f32],
                coordinate_bias: [(x0 as f32 + bias_off) / w as f32, (y0 as f32 + bias_off) / h as f32],
            };
            let mut obj = ed.object(a.component)?.clone();
            let Native::StaticMeshComponent(smc) = &mut obj.native else {
                return Err(Error::Format(format!("export {} is not a static mesh component", a.component)));
            };
            while smc.lod_data.len() <= a.lod {
                smc.lod_data.push(StaticMeshComponentLod::default());
            }
            let l = &mut smc.lod_data[a.lod];
            l.light_map = lm;
            l.shadow_maps.clear();
            l.shadow_vertex_buffers.clear();
            ed.replace_object(a.component, obj)?;
            if a.lod == 0 {
                written.components += 1;
            }
        }
    }
    // LODs not baked (no light map UVs) would keep light maps of the
    // original's place.
    let mut components: Vec<usize> = allocs.iter().map(|a| a.component).collect();
    components.sort_unstable();
    components.dedup();
    for c in components {
        let mut obj = ed.object(c)?.clone();
        let Native::StaticMeshComponent(smc) = &mut obj.native else { continue };
        let mut cleared = 0;
        for (lod, l) in smc.lod_data.iter_mut().enumerate() {
            if l.light_map != LightMap::None && !allocs.iter().any(|a| a.component == c && a.lod == lod) {
                l.light_map = LightMap::None;
                cleared += 1;
            }
        }
        if cleared > 0 {
            written.cleared_lods += cleared;
            ed.replace_object(c, obj)?;
        }
    }
    written.removed = remove_unused(ed, old)?;
    Ok(written)
}

/// Delete the exports of `candidates` that no export outside the ones
/// being deleted refers to (repeated until stable: a shadow map texture
/// goes with the last shadow map using it). Returns their paths.
fn remove_unused(ed: &mut Editor, mut candidates: Vec<usize>) -> Result<Vec<String>> {
    loop {
        let set: std::collections::HashSet<PackageIndex> = candidates.iter().map(|&i| PackageIndex::export(i)).collect();
        let mut used = std::collections::HashSet::new();
        for j in 0..ed.pkg.exports.len() {
            if set.contains(&PackageIndex::export(j)) {
                continue;
            }
            let e = &ed.pkg.exports[j];
            used.extend([e.class, e.super_, e.outer, e.archetype].into_iter().filter(|r| set.contains(r)));
            if let Ok(Parsed::Object(o)) = ed.parsed(j) {
                used.extend(references(o).into_iter().filter(|r| set.contains(r)));
            }
        }
        let before = candidates.len();
        candidates.retain(|&i| !used.contains(&PackageIndex::export(i)));
        if candidates.len() == before {
            break;
        }
    }
    let paths = candidates.iter().map(|&i| ed.pkg.object_path(PackageIndex::export(i))).collect();
    // From the end, so the indices of the others hold.
    for &i in candidates.iter().rev() {
        // Gone already with its outer.
        if !matches!(ed.parsed(i), Ok(Parsed::Empty)) {
            ed.delete(i)?;
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::pack;

    #[test]
    fn packs_without_overlap() {
        let sizes = [130, 66, 66, 34, 34, 34, 18, 1000, 600];
        let p = pack(&sizes, 1024);
        for i in 0..sizes.len() {
            for j in i + 1..sizes.len() {
                if p[i].0 != p[j].0 {
                    continue;
                }
                let (a, b) = (p[i], p[j]);
                let overlap = a.1 < b.1 + sizes[j] && b.1 < a.1 + sizes[i] && a.2 < b.2 + sizes[j] && b.2 < a.2 + sizes[i];
                assert!(!overlap, "{i} {j} {p:?}");
            }
            assert!(p[i].1 % 4 == 0 && p[i].2 % 4 == 0);
        }
    }
}
