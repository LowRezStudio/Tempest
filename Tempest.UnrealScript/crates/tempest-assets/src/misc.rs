//! Small native payloads: redirectors, the GUID cache, sound cues, fonts,
//! 1-D shadow maps and sound waves.

use tempest_upk::io::{FName, Guid, PackageIndex, Reader, Result, Writer};

use crate::bulk::BulkData;

/// `UObjectRedirector::Serialize`: `DestinationObject`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectRedirector {
    pub destination: PackageIndex,
}

impl ObjectRedirector {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(ObjectRedirector { destination: r.index()? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.index(self.destination);
    }
}

/// `UGuidCache::Serialize`: `TMap<FName, FGuid> PackageGuidMap`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuidCache {
    pub packages: Vec<(FName, Guid)>,
}

impl GuidCache {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(GuidCache { packages: r.array(|r| Ok((r.fname()?, r.guid()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.packages, |w, (n, g)| {
            w.fname(*n);
            w.guid(g);
        });
    }
}

/// `USoundCue::Serialize`: `TMap<USoundNode*, FSoundNodeEditorData>`
/// (`NodePosX`, `NodePosY`), always empty in cooked packages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundCue {
    pub editor_data: Vec<(PackageIndex, i32, i32)>,
}

impl SoundCue {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(SoundCue { editor_data: r.array(|r| Ok((r.index()?, r.i32()?, r.i32()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.editor_data, |w, (n, x, y)| {
            w.index(*n);
            w.i32(*x);
            w.i32(*y);
        });
    }
}

/// `UFont::Serialize` (and `UMultiFont`): `TMap<WORD, WORD> CharRemap`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Font {
    pub char_remap: Vec<(u16, u16)>,
}

impl Font {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(Font { char_remap: r.array(|r| Ok((r.u16()?, r.u16()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.char_remap, |w, (a, b)| {
            w.u16(*a);
            w.u16(*b);
        });
    }
}

/// `UShadowMap1D::Serialize`: `TResourceArray<FShadowSample>` (one float
/// visibility per vertex in this build, 4 bytes each) and `LightGuid`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShadowMap1D {
    pub samples: Vec<f32>,
    pub light_guid: Guid,
}

impl ShadowMap1D {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let samples = r.array(|r| r.f32())?;
        Ok(ShadowMap1D { samples, light_guid: r.guid()? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.samples, |w, s| w.f32(*s));
        w.guid(&self.light_guid);
    }
}

/// Names of the bulk data members of `USoundNodeWave` in serialisation
/// order (file version 893; from the 8.2 binary).
pub const SOUND_WAVE_BULKS: [&str; 9] = [
    "RawData",
    "CompressedPCData",
    "CompressedXbox360Data",
    "CompressedPS3Data",
    "CompressedWiiUData",
    "CompressedIPhoneData",
    "CompressedFlashData",
    "CompressedAndroidData",
    "CompressedOrbisData",
];

/// `USoundNodeWave::Serialize`: nine bulk data members.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundNodeWave {
    pub bulks: Vec<BulkData>,
}

impl SoundNodeWave {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let mut bulks = Vec::with_capacity(SOUND_WAVE_BULKS.len());
        for _ in SOUND_WAVE_BULKS {
            bulks.push(BulkData::read(r)?);
        }
        Ok(SoundNodeWave { bulks })
    }
    pub fn write(&self, w: &mut Writer) {
        for b in &self.bulks {
            b.write(w);
        }
    }
    pub fn bulk(&self, name: &str) -> Option<&BulkData> {
        SOUND_WAVE_BULKS.iter().position(|n| n.eq_ignore_ascii_case(name)).and_then(|i| self.bulks.get(i))
    }
    pub fn bulk_mut(&mut self, name: &str) -> Option<&mut BulkData> {
        SOUND_WAVE_BULKS.iter().position(|n| n.eq_ignore_ascii_case(name)).and_then(|i| self.bulks.get_mut(i))
    }
    /// The PC payload (Ogg Vorbis in this game), expanded.
    pub fn pc_data(&self, obscured: bool) -> Result<Option<Vec<u8>>> {
        match self.bulk("CompressedPCData") {
            Some(b) => b.inline_data(obscured),
            None => Ok(None),
        }
    }
}
