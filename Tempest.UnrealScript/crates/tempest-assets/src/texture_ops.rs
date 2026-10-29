//! Texture-level operations over a parsed object: describe, export to
//! DDS/PNG, replace pixels.

use tempest_upk::flags::compression;
use tempest_upk::io::{Error, PackageIndex, Result};
use tempest_upk::Package;

use crate::dds::{self, DdsMip, PixelFormat};
use crate::object::{Native, Object};
use crate::reflect::ClassDb;
use crate::texcache::{MipLocation, TextureSource};
use crate::value::Value;

/// `UObject::GetPathName()` of an export: `Package.Outer.Name`, where an
/// object copied into a seek-free package keeps its original package (the
/// outermost `Package` export) as the first component.
pub fn engine_path(pkg: &Package, pkg_name: &str, i: usize) -> String {
    let idx = PackageIndex::export(i);
    let mut cur = idx;
    let mut guard = 0;
    while !pkg.object_outer(cur).is_null() && guard < 64 {
        cur = pkg.object_outer(cur);
        guard += 1;
    }
    let forced = cur.is_export() && pkg.class_name_of(cur).eq_ignore_ascii_case("Package");
    if forced {
        pkg.object_path(idx)
    } else {
        format!("{pkg_name}.{}", pkg.object_path(idx))
    }
}

#[derive(Clone, Debug)]
pub struct MipInfo {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub location: MipLocation,
    pub stored_bytes: i32,
}

#[derive(Clone, Debug)]
pub struct TextureInfo {
    pub path: String,
    pub format_name: String,
    pub format: Option<PixelFormat>,
    pub width: u32,
    pub height: u32,
    pub mips: Vec<MipInfo>,
    pub lightmap: bool,
}

/// The texture's `Format` property, falling back to the class default object.
pub fn format_name(db: &ClassDb, pkg: &Package, obj: &Object) -> String {
    if let Some(p) = obj.property("Format", pkg) {
        if let Value::Enum(n) = &p.value {
            return pkg.fname_to_string(*n);
        }
    }
    if let Some(class) = obj.class {
        if let Some(v) = db.cdo_enum_property(class, "Format") {
            return v;
        }
    }
    "PF_Unknown".to_string()
}

fn int_property(pkg: &Package, obj: &Object, name: &str) -> Option<i32> {
    match obj.property(name, pkg).map(|p| &p.value) {
        Some(Value::Int(i)) => Some(*i),
        _ => None,
    }
}

pub fn texture_info(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object) -> Result<TextureInfo> {
    let Native::Texture2D(t) = &obj.native else {
        return Err(Error::Format(format!("{} is not a Texture2D", pkg.full_name(PackageIndex::export(i)))));
    };
    let path = engine_path(pkg, pkg_name, i);
    let format_name = format_name(db, pkg, obj);
    let format = PixelFormat::from_name(&format_name);
    let width = int_property(pkg, obj, "SizeX").unwrap_or_else(|| t.mips.first().map(|m| m.size_x).unwrap_or(0)) as u32;
    let height = int_property(pkg, obj, "SizeY").unwrap_or_else(|| t.mips.first().map(|m| m.size_y).unwrap_or(0)) as u32;
    let mips = t
        .mips
        .iter()
        .enumerate()
        .map(|(mi, m)| MipInfo { index: mi, width: m.size_x as u32, height: m.size_y as u32, location: src.locate(&path, mi, m), stored_bytes: m.data.size_on_disk })
        .collect();
    Ok(TextureInfo { path, format_name, format, width, height, mips, lightmap: t.lightmap_flags.is_some() })
}

/// All available mips (from the largest one that has data down to the
/// smallest), expanded.
pub fn available_mips(pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object) -> Result<Vec<(u32, u32, Vec<u8>)>> {
    let Native::Texture2D(t) = &obj.native else {
        return Err(Error::Format("not a Texture2D".into()));
    };
    let path = engine_path(pkg, pkg_name, i);
    let obscured = pkg.summary.compression_flags & compression::OBSCURED != 0;
    let mut out = Vec::new();
    for (mi, m) in t.mips.iter().enumerate() {
        match src.mip_data(&path, mi, m, obscured) {
            Ok(Some(d)) => out.push((m.size_x as u32, m.size_y as u32, d)),
            Ok(None) => {
                if !out.is_empty() {
                    break;
                }
            }
            Err(e) => {
                if out.is_empty() {
                    return Err(e);
                }
                break;
            }
        }
    }
    if out.is_empty() {
        return Err(Error::Format(format!("{path}: no mip has data")));
    }
    Ok(out)
}

pub fn export_dds(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object) -> Result<Vec<u8>> {
    let fname = format_name(db, pkg, obj);
    let format = PixelFormat::from_name(&fname).ok_or_else(|| Error::Format(format!("unsupported pixel format {fname}")))?;
    let mips = available_mips(pkg, pkg_name, src, i, obj)?;
    let refs: Vec<DdsMip> = mips.iter().map(|(w, h, d)| DdsMip { width: *w, height: *h, data: d }).collect();
    dds::write_dds(format, &refs)
}

/// PNG of the largest available mip (or of mip `mip` counted from it).
pub fn export_png(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object, mip: usize) -> Result<Vec<u8>> {
    let fname = format_name(db, pkg, obj);
    let format = PixelFormat::from_name(&fname).ok_or_else(|| Error::Format(format!("unsupported pixel format {fname}")))?;
    let mips = available_mips(pkg, pkg_name, src, i, obj)?;
    let (w, h, d) = mips.get(mip).ok_or_else(|| Error::Format(format!("only {} mips available", mips.len())))?;
    let rgba = dds::decode_rgba(format, *w, *h, d)?;
    dds::write_png(*w, *h, &rgba)
}

/// PNG of the largest available mip no wider/taller than `max_size`
/// (fast compression): preview quality, a fraction of the full-size cost.
pub fn export_png_preview(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object, max_size: u32) -> Result<Vec<u8>> {
    let fname = format_name(db, pkg, obj);
    let format = PixelFormat::from_name(&fname).ok_or_else(|| Error::Format(format!("unsupported pixel format {fname}")))?;
    let mips = available_mips(pkg, pkg_name, src, i, obj)?;
    let pick = mips.iter().position(|(w, h, _)| *w <= max_size && *h <= max_size).unwrap_or(mips.len().saturating_sub(1));
    let (w, h, d) = mips.get(pick).ok_or_else(|| Error::Format("no mips available".into()))?;
    let rgba = dds::decode_rgba(format, *w, *h, d)?;
    dds::write_png_with(*w, *h, &rgba, true)
}

/// RGBA8 pixels of the largest available mip (no PNG encoding), for embedding
/// a texture into a movie the player can render.
pub fn largest_rgba(db: &ClassDb, pkg: &Package, pkg_name: &str, src: &TextureSource, i: usize, obj: &Object) -> Result<(u32, u32, Vec<u8>)> {
    let fname = format_name(db, pkg, obj);
    let format = PixelFormat::from_name(&fname).ok_or_else(|| Error::Format(format!("unsupported pixel format {fname}")))?;
    let mips = available_mips(pkg, pkg_name, src, i, obj)?;
    let (w, h, d) = mips.first().ok_or_else(|| Error::Format("no mips available".into()))?;
    Ok((*w, *h, dds::decode_rgba(format, *w, *h, d)?))
}

// ----------------------------------------------------------------------------
// Import
// ----------------------------------------------------------------------------

use tempest_unrealscript::objects::PropertyKind;
use crate::bulk::BulkData;
use crate::texture::Mip;

/// New pixels for a texture.
pub enum ImportSource {
    /// Already encoded mips (mip 0 first) in `format`.
    Encoded { format: PixelFormat, width: u32, height: u32, mips: Vec<Vec<u8>> },
    /// RGBA8 pixels to encode in the texture's format (or `format`).
    Rgba { width: u32, height: u32, rgba: Vec<u8> },
}

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// Generate the full mip chain (`Some(false)`: a single mip, like
    /// `TMGS_NoMipmaps`; `None`: as many mips as the texture had).
    pub mips: Option<bool>,
    /// Encode RGBA input in this format instead of the texture's current one.
    pub format: Option<PixelFormat>,
    /// RGBA input with translucent pixels going to a DXT1 texture (no alpha
    /// beyond a 1-bit cut-out) is encoded as DXT5 instead.
    pub keep_alpha: bool,
    /// RGBA input whose size is not a whole number of 4x4 blocks going to a
    /// block-compressed format is padded on the right and bottom (as the GFx
    /// exporter does: the movie's image tag keeps the real size) instead of
    /// scaled. D3D9 refuses DXT textures of any other size.
    pub pad_blocks: bool,
}

use crate::media::{remove_property, set_property};

/// Replace the pixels of texture export `i` (`obj` must be its parsed
/// object) and store every mip inline, so the package needs no texture
/// file cache. Updates `SizeX`/`SizeY`/`OriginalSizeX`/`OriginalSizeY`,
/// `Format`, `MipTailBaseIdx`, `FirstResourceMemMip` and `NeverStream`, and
/// drops the export from the summary's texture pre-allocations (console
/// RHIs only).
pub fn import(db: &ClassDb, pkg: &mut Package, i: usize, obj: &mut Object, source: ImportSource, opts: &ImportOptions) -> Result<()> {
    let current = format_name(db, pkg, obj);
    let had_mips = matches!(&obj.native, Native::Texture2D(t) if t.mips.len() > 1);
    let want_mips = opts.mips.unwrap_or(had_mips);
    let (format, width, height, mips): (PixelFormat, u32, u32, Vec<Vec<u8>>) = match source {
        ImportSource::Encoded { format, width, height, mips } => {
            if format.is_block_compressed() && (!width.is_multiple_of(4) || !height.is_multiple_of(4)) {
                return Err(Error::Format(format!(
                    "{width}x{height} {}: the game cannot create a block-compressed texture whose size is not a multiple of 4; use a PNG or a DDS of {}x{}",
                    format.name(),
                    width.next_multiple_of(4),
                    height.next_multiple_of(4)
                )));
            }
            let mips = if want_mips { mips } else { mips.into_iter().take(1).collect() };
            (format, width, height, mips)
        }
        ImportSource::Rgba { width, height, rgba } => {
            let mut format = opts.format.or_else(|| PixelFormat::from_name(&current)).ok_or_else(|| Error::Format(format!("texture format {current} is not supported; pass an explicit format")))?;
            if opts.keep_alpha && opts.format.is_none() && format == PixelFormat::Dxt1 && rgba.as_chunks::<4>().0.iter().any(|px| px[3] < 255) {
                format = PixelFormat::Dxt5;
            }
            // D3D9's CreateTexture fails (D3DERR_INVALIDCALL, a fatal error in
            // game) for a DXT texture whose size is not a multiple of 4.
            let (width, height, rgba) = if format.is_block_compressed() && (!width.is_multiple_of(4) || !height.is_multiple_of(4)) {
                let (pw, ph) = (width.next_multiple_of(4), height.next_multiple_of(4));
                // A texture already that padded size holds a GFx image of the new image's size.
                let padded_before = int_property(pkg, obj, "SizeX") == Some(pw as i32) && int_property(pkg, obj, "SizeY") == Some(ph as i32);
                if opts.pad_blocks || padded_before {
                    (pw, ph, dds::pad_rgba(width, height, &rgba, pw, ph))
                } else {
                    let (nw, nh) = ((width + 2) / 4 * 4, (height + 2) / 4 * 4);
                    let (nw, nh) = (nw.max(4), nh.max(4));
                    (nw, nh, dds::resize_rgba(width, height, &rgba, nw, nh))
                }
            } else {
                (width, height, rgba)
            };
            let chain = if want_mips { dds::rgba_mip_chain(width, height, &rgba) } else { vec![(width, height, rgba)] };
            let mut encoded = Vec::with_capacity(chain.len());
            for (w, h, px) in &chain {
                encoded.push(dds::encode_rgba(format, *w, *h, px)?);
            }
            (format, width, height, encoded)
        }
    };
    if width == 0 || height == 0 || mips.is_empty() {
        return Err(Error::Format("empty image".into()));
    }
    let min = if format.is_block_compressed() { 4 } else { 1 };
    let mut new_mips = Vec::with_capacity(mips.len());
    for (mi, data) in mips.iter().enumerate() {
        let w = (width >> mi).max(min);
        let h = (height >> mi).max(min);
        let want = format.image_bytes(w, h);
        if data.len() != want {
            return Err(Error::Format(format!("mip {mi} ({w}x{h}): {} bytes, expected {want} for {}", data.len(), format.name())));
        }
        let mut bulk = BulkData::default();
        bulk.set_inline(data, false, false);
        new_mips.push(Mip { data: bulk, size_x: w as i32, size_y: h as i32 });
    }
    let Native::Texture2D(t) = &mut obj.native else {
        return Err(Error::Format("not a Texture2D".into()));
    };
    t.mips = new_mips;
    t.cached_pvrtc_mips.clear();
    t.cached_atitc_mips.clear();
    t.cached_etc_mips.clear();
    let count = t.mips.len() as i32;

    set_property(pkg, obj, "SizeX", PropertyKind::Int, Value::Int(width as i32));
    set_property(pkg, obj, "SizeY", PropertyKind::Int, Value::Int(height as i32));
    if obj.property("OriginalSizeX", pkg).is_some() || obj.property("OriginalSizeY", pkg).is_some() {
        set_property(pkg, obj, "OriginalSizeX", PropertyKind::Int, Value::Int(width as i32));
        set_property(pkg, obj, "OriginalSizeY", PropertyKind::Int, Value::Int(height as i32));
    }
    if !current.eq_ignore_ascii_case(format.name()) {
        let n = pkg.intern_fname(format.name());
        set_property(pkg, obj, "Format", PropertyKind::Byte, Value::Enum(n));
        let e = pkg.intern_fname("EPixelFormat");
        if let Some(p) = obj.properties.iter_mut().find(|p| pkg.fname_to_string(p.name).eq_ignore_ascii_case("Format")) {
            p.enum_name = Some(e);
        }
    }
    if count > 1 {
        set_property(pkg, obj, "MipTailBaseIdx", PropertyKind::Int, Value::Int(count - 1));
    } else {
        remove_property(pkg, obj, "MipTailBaseIdx");
    }
    remove_property(pkg, obj, "FirstResourceMemMip");
    set_property(pkg, obj, "NeverStream", PropertyKind::Bool, Value::Bool(true));

    // Summary pre-allocation groups (unused by the PC RHI, kept consistent).
    for t in &mut pkg.summary.texture_allocations {
        t.export_indices.retain(|&e| e != i as i32);
    }
    pkg.summary.texture_allocations.retain(|t| !t.export_indices.is_empty());
    Ok(())
}
