//! Scaleform movies and sound waves: extract and replace their payloads.
//!
//! * `SwfMovie` (`GFxRawData`) keeps the compiled movie in the script
//!   property `RawData` (`array<byte>`), a GFx file (`GFX`/`CFX` header).
//! * `SoundNodeWave` keeps the PC audio in the `CompressedPCData` bulk
//!   member: Ogg Vorbis in this game. `Duration`, `NumChannels`,
//!   `SampleRate` and `RawPCMDataSize` are script properties the engine
//!   reads back, so a replacement updates them from the Ogg headers.

use tempest_unrealscript::objects::PropertyKind;
use tempest_upk::flags::compression;
use tempest_upk::io::{Error, Result};
use tempest_upk::Package;

use crate::bulk::BulkData;
use crate::object::{Native, Object};
use crate::value::Value;

// ----------------------------------------------------------------------------
// Properties
// ----------------------------------------------------------------------------

/// Bytes of an `array<byte>` property.
pub fn byte_array_property(pkg: &Package, obj: &Object, name: &str) -> Option<Vec<u8>> {
    match &obj.property(name, pkg)?.value {
        Value::Array(items) => items.iter().map(|v| if let Value::Byte(b) = v { Some(*b) } else { None }).collect(),
        Value::Raw(raw) if raw.len() >= 4 => {
            let n = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]).max(0) as usize;
            raw.get(4..4 + n).map(|b| b.to_vec())
        }
        _ => None,
    }
}

/// Set (or add) a scalar/array property by name.
pub fn set_property(pkg: &mut Package, obj: &mut Object, name: &str, kind: PropertyKind, value: Value) {
    crate::particle::set_member(pkg, &mut obj.properties, name, kind, value);
}

pub fn remove_property(pkg: &Package, obj: &mut Object, name: &str) {
    obj.properties.retain(|p| !pkg.fname_to_string(p.name).eq_ignore_ascii_case(name));
}

pub fn set_byte_array_property(pkg: &mut Package, obj: &mut Object, name: &str, bytes: &[u8]) {
    let value = Value::Array(bytes.iter().map(|b| Value::Byte(*b)).collect());
    set_property(pkg, obj, name, PropertyKind::Array, value);
}

// ----------------------------------------------------------------------------
// SwfMovie
// ----------------------------------------------------------------------------

pub fn swf_export(pkg: &Package, obj: &Object) -> Result<Vec<u8>> {
    byte_array_property(pkg, obj, "RawData").ok_or_else(|| Error::Format("no RawData property (not a SwfMovie, or empty)".into()))
}

/// Replace the movie. Accepts a GFx (`GFX`/`CFX`) or plain SWF (`FWS`/`CWS`)
/// file; the engine's GFx loader reads both.
pub fn swf_import(pkg: &mut Package, obj: &mut Object, bytes: &[u8]) -> Result<()> {
    let magic = bytes.get(0..3).unwrap_or(&[]);
    if !matches!(magic, b"GFX" | b"CFX" | b"FWS" | b"CWS" | b"ZWS") {
        return Err(Error::Format("not a GFx or SWF file (bad signature)".into()));
    }
    set_byte_array_property(pkg, obj, "RawData", bytes);
    Ok(())
}

// ----------------------------------------------------------------------------
// Ogg Vorbis
// ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OggInfo {
    pub channels: u8,
    pub sample_rate: u32,
    /// Total PCM frames (from the last page's granule position).
    pub frames: u64,
}

impl OggInfo {
    pub fn duration(&self) -> f32 {
        if self.sample_rate == 0 { 0.0 } else { self.frames as f32 / self.sample_rate as f32 }
    }
}

/// Channel count, rate and length of an Ogg Vorbis stream.
pub fn ogg_info(bytes: &[u8]) -> Result<OggInfo> {
    if bytes.len() < 58 || &bytes[0..4] != b"OggS" {
        return Err(Error::Format("not an Ogg stream".into()));
    }
    // First page: header 27 bytes + segment table, then the identification packet.
    let nseg = bytes[26] as usize;
    let body = 27 + nseg;
    let id = bytes.get(body..body + 30).ok_or_else(|| Error::Format("truncated Ogg header".into()))?;
    if id[0] != 1 || &id[1..7] != b"vorbis" {
        return Err(Error::Format("Ogg stream is not Vorbis".into()));
    }
    let channels = id[11];
    let sample_rate = u32::from_le_bytes([id[12], id[13], id[14], id[15]]);
    // Last page: scan backwards for a capture pattern and take its granule position.
    let mut frames = 0u64;
    let mut pos = bytes.len().saturating_sub(27);
    while pos > 0 {
        if &bytes[pos..pos + 4] == b"OggS" && bytes[pos + 4] == 0 {
            let g = &bytes[pos + 6..pos + 14];
            let v = u64::from_le_bytes([g[0], g[1], g[2], g[3], g[4], g[5], g[6], g[7]]);
            if v != u64::MAX {
                frames = v;
                break;
            }
        }
        pos -= 1;
    }
    Ok(OggInfo { channels, sample_rate, frames })
}

// ----------------------------------------------------------------------------
// SoundNodeWave
// ----------------------------------------------------------------------------

pub fn sound_export(pkg: &Package, obj: &Object) -> Result<Vec<u8>> {
    let Native::SoundNodeWave(s) = &obj.native else {
        return Err(Error::Format("not a SoundNodeWave".into()));
    };
    let obscured = pkg.summary.compression_flags & compression::OBSCURED != 0;
    match s.pc_data(obscured)? {
        Some(d) if !d.is_empty() => Ok(d),
        _ => Err(Error::Format("the wave has no PC data (it may be a streaming wave or stripped)".into())),
    }
}

/// Replace the PC audio with an Ogg Vorbis file (stored inline, raw, as the
/// cooker does), clear the editor-only `RawData`, and update the
/// descriptive properties.
pub fn sound_import(pkg: &mut Package, obj: &mut Object, ogg: &[u8]) -> Result<()> {
    let info = ogg_info(ogg)?;
    let Native::SoundNodeWave(s) = &mut obj.native else {
        return Err(Error::Format("not a SoundNodeWave".into()));
    };
    let pc = s.bulk_mut("CompressedPCData").ok_or_else(|| Error::Format("no CompressedPCData".into()))?;
    pc.set_inline(ogg, false, false);
    if let Some(raw) = s.bulk_mut("RawData") {
        *raw = BulkData { flags: raw.flags & !(crate::bulk::flags::STORE_IN_SEPARATE_FILE | crate::bulk::flags::SERIALIZE_COMPRESSED), element_count: 0, size_on_disk: 0, offset_in_file: -1, inline: Some(Vec::new()) };
    }
    set_property(pkg, obj, "Duration", PropertyKind::Float, Value::Float(info.duration()));
    set_property(pkg, obj, "NumChannels", PropertyKind::Int, Value::Int(info.channels as i32));
    set_property(pkg, obj, "SampleRate", PropertyKind::Int, Value::Int(info.sample_rate as i32));
    set_property(pkg, obj, "RawPCMDataSize", PropertyKind::Int, Value::Int((info.frames * info.channels as u64 * 2) as i32));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ogg_headers() {
        // Page 0: one segment of 30 bytes holding the identification header.
        let mut v = Vec::new();
        v.extend_from_slice(b"OggS");
        v.extend_from_slice(&[0, 2]); // version, header type (BOS)
        v.extend_from_slice(&0u64.to_le_bytes()); // granule
        v.extend_from_slice(&[0; 4]); // serial
        v.extend_from_slice(&[0; 4]); // page seq
        v.extend_from_slice(&[0; 4]); // crc
        v.push(1); // segments
        v.push(30);
        v.push(1);
        v.extend_from_slice(b"vorbis");
        v.extend_from_slice(&0u32.to_le_bytes()); // version
        v.push(2); // channels
        v.extend_from_slice(&44100u32.to_le_bytes());
        v.extend_from_slice(&[0; 13]);
        // A last page with granule 88200.
        v.extend_from_slice(b"OggS");
        v.extend_from_slice(&[0, 4]);
        v.extend_from_slice(&88200u64.to_le_bytes());
        v.extend_from_slice(&[0; 13]);
        let info = ogg_info(&v).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.frames, 88200);
        assert_eq!(info.duration(), 2.0);
    }
}
