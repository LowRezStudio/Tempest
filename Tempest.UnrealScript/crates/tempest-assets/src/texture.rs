//! Native payloads of the texture classes.
//!
//! * `UTexture::Serialize` (Texture.cpp): `SourceArt` bulk data.
//! * `UTexture2D::Serialize` (Texture2D.cpp, confirmed in the 8.2 binary):
//!   `Mips` (`TIndirectArray<FTexture2DMipMap>`: count, then per mip the
//!   bulk data followed by `SizeX`, `SizeY`), `TextureFileCacheGuid`,
//!   `CachedPVRTCMips`, `CachedFlashMipsMaxResolution`, `CachedATITCMips`,
//!   `CachedFlashMips` (bulk), `CachedETCMips`.
//! * `ULightMapTexture2D::Serialize`: the above, then `LightmapFlags`.
//! * `UTextureMovie::Serialize`: `UTexture` then `Data` bulk.
//! * `UTextureCube`, `UTextureRenderTarget*`, `UShadowMapTexture2D`,
//!   `UTextureFlipBook`: nothing beyond their parent.
//!
//! Hi-Rez sets `TextureFileCacheName` to `Automated` while cooking; which
//! `.tfc` holds a streamed mip comes from `MergedFileIndexCache.bin`.

use tempest_upk::io::{Guid, Reader, Result, Writer};

use crate::bulk::BulkData;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mip {
    pub data: BulkData,
    pub size_x: i32,
    pub size_y: i32,
}

impl Mip {
    pub fn read(r: &mut Reader) -> Result<Mip> {
        Ok(Mip { data: BulkData::read(r)?, size_x: r.i32()?, size_y: r.i32()? })
    }

    pub fn write(&self, w: &mut Writer) {
        self.data.write(w);
        w.i32(self.size_x);
        w.i32(self.size_y);
    }
}

fn read_mips(r: &mut Reader) -> Result<Vec<Mip>> {
    r.array(Mip::read)
}

fn write_mips(w: &mut Writer, mips: &[Mip]) {
    w.array(mips, |w, m| m.write(w));
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Texture2D {
    pub source_art: BulkData,
    pub mips: Vec<Mip>,
    pub file_cache_guid: Guid,
    pub cached_pvrtc_mips: Vec<Mip>,
    pub cached_flash_mips_max_resolution: i32,
    pub cached_atitc_mips: Vec<Mip>,
    pub cached_flash_mips: BulkData,
    pub cached_etc_mips: Vec<Mip>,
    /// `ULightMapTexture2D` only.
    pub lightmap_flags: Option<u32>,
}

impl Texture2D {
    pub fn read(r: &mut Reader, lightmap: bool) -> Result<Texture2D> {
        let source_art = BulkData::read(r)?;
        let mips = read_mips(r)?;
        let file_cache_guid = r.guid()?;
        let cached_pvrtc_mips = read_mips(r)?;
        let cached_flash_mips_max_resolution = r.i32()?;
        let cached_atitc_mips = read_mips(r)?;
        let cached_flash_mips = BulkData::read(r)?;
        let cached_etc_mips = read_mips(r)?;
        let lightmap_flags = if lightmap { Some(r.u32()?) } else { None };
        Ok(Texture2D {
            source_art,
            mips,
            file_cache_guid,
            cached_pvrtc_mips,
            cached_flash_mips_max_resolution,
            cached_atitc_mips,
            cached_flash_mips,
            cached_etc_mips,
            lightmap_flags,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        self.source_art.write(w);
        write_mips(w, &self.mips);
        w.guid(&self.file_cache_guid);
        write_mips(w, &self.cached_pvrtc_mips);
        w.i32(self.cached_flash_mips_max_resolution);
        write_mips(w, &self.cached_atitc_mips);
        self.cached_flash_mips.write(w);
        write_mips(w, &self.cached_etc_mips);
        if let Some(f) = self.lightmap_flags {
            w.u32(f);
        }
    }

    /// Mips that carry data (not flagged unused), in order.
    pub fn used_mips(&self) -> impl Iterator<Item = (usize, &Mip)> {
        self.mips.iter().enumerate().filter(|(_, m)| !m.data.is_unused())
    }
}

/// `UTexture` (cubes, render targets): only `SourceArt`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureBase {
    pub source_art: BulkData,
}

impl TextureBase {
    pub fn read(r: &mut Reader) -> Result<TextureBase> {
        Ok(TextureBase { source_art: BulkData::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.source_art.write(w);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureMovie {
    pub source_art: BulkData,
    pub data: BulkData,
}

impl TextureMovie {
    pub fn read(r: &mut Reader) -> Result<TextureMovie> {
        Ok(TextureMovie { source_art: BulkData::read(r)?, data: BulkData::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.source_art.write(w);
        self.data.write(w);
    }
}
