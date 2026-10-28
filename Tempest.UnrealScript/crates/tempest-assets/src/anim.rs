//! Skeletal animations (`UAnimSequence`): the compressed key stream decoded
//! into tracks, sampled into poses, and encoded back.
//!
//! A cooked sequence keeps no raw tracks: its motion is `CompressedByteStream`
//! (the native part, [`crate::level::AnimSequence::compressed`]) described by
//! properties: `NumFrames`, `SequenceLength`, `KeyEncodingFormat`,
//! `RotationCompressionFormat`, `TranslationCompressionFormat` and
//! `CompressedTrackOffsets` (per track: translation offset, translation key
//! count, rotation offset, rotation key count). Tracks follow the owning
//! `UAnimSet`'s `TrackBoneNames`.
//!
//! Ported from the 10897 source (`AnimationCompression.h`,
//! `AnimationEncodingFormat*.cpp`, `AnimationCompressionAlgorithm.cpp`) and
//! checked against the 8.2 server binary (`AEFVariableKeyLerp<N>`): the
//! layout is Epic's. Per track, translation then rotation, each padded to 4
//! bytes with 0x55: a single key is a full `FVector` / `FQuatFloat96NoW`
//! whatever the format; several keys are `[mins, ranges]` (interval formats),
//! the keys, and with `AKF_VariableKeyLerp` a frame table (one byte per key
//! when `NumFrames <= 255`, else two) padded with 0x55 (zeros in the source). Every shipped
//! sequence uses `AKF_VariableKeyLerp` + `ACF_Float96NoW` rotations + `ACF_None`
//! translations, which is what [`encode`] writes.

use tempest_upk::io::{Error, Result};

/// `AnimationKeyFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFormat {
    ConstantKeyLerp,
    VariableKeyLerp,
    PerTrackCompression,
}

/// `AnimationCompressionFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acf {
    None,
    Float96NoW,
    Fixed48NoW,
    IntervalFixed32NoW,
    Fixed32NoW,
    Float32NoW,
    Identity,
}

impl Acf {
    /// From the enum's tag name (`ACF_Float96NoW`, any case) or `None` when absent.
    pub fn parse(name: Option<&str>) -> Acf {
        match name.map(|n| n.to_ascii_lowercase()) {
            Some(n) if n.ends_with("float96now") => Acf::Float96NoW,
            Some(n) if n.ends_with("fixed48now") => Acf::Fixed48NoW,
            Some(n) if n.ends_with("intervalfixed32now") => Acf::IntervalFixed32NoW,
            Some(n) if n.ends_with("fixed32now") => Acf::Fixed32NoW,
            Some(n) if n.ends_with("float32now") => Acf::Float32NoW,
            Some(n) if n.ends_with("identity") => Acf::Identity,
            _ => Acf::None,
        }
    }
}

impl KeyFormat {
    pub fn parse(name: Option<&str>) -> KeyFormat {
        match name.map(|n| n.to_ascii_lowercase()) {
            Some(n) if n.ends_with("variablekeylerp") => KeyFormat::VariableKeyLerp,
            Some(n) if n.ends_with("pertrackcompression") => KeyFormat::PerTrackCompression,
            _ => KeyFormat::ConstantKeyLerp,
        }
    }
}

/// What a sequence's properties say about its stream.
#[derive(Clone, Debug)]
pub struct Header {
    pub num_frames: usize,
    pub length: f32,
    pub key_format: KeyFormat,
    pub rot_format: Acf,
    pub trans_format: Acf,
    pub offsets: Vec<i32>,
}

/// One bone's keys. `*_frames` are the frame positions of the keys (whole
/// frames for variable keys; spread evenly for constant keys).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub trans: Vec<[f32; 3]>,
    pub trans_frames: Vec<f32>,
    /// Quaternions (x, y, z, w), `w >= 0`.
    pub rot: Vec<[f32; 4]>,
    pub rot_frames: Vec<f32>,
}

/// A decoded sequence.
#[derive(Clone, Debug, Default)]
pub struct Anim {
    pub num_frames: usize,
    pub length: f32,
    pub tracks: Vec<Track>,
}

const PAD: u8 = 0x55;

fn rd_f32(b: &[u8], at: usize) -> Result<f32> {
    b.get(at..at + 4).map(|s| f32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| Error::Format("animation stream too short".into()))
}

fn rd_u32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| Error::Format("animation stream too short".into()))
}

fn rd_u16(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| Error::Format("animation stream too short".into()))
}

fn w_from_xyz(x: f32, y: f32, z: f32) -> [f32; 4] {
    let w2 = 1.0 - x * x - y * y - z * z;
    [x, y, z, if w2 > 0.0 { w2.sqrt() } else { 0.0 }]
}

/// `TFloatPacker<3, M, TRUE>::Decode`.
fn float_unpack(v: u32, mantissa_bits: u32) -> f32 {
    if v == 0 {
        return 0.0;
    }
    let exp_bits = 3;
    let bias = (1 << (exp_bits - 1)) - 1;
    let min_exp = -bias - 1;
    let mantissa = v & ((1 << mantissa_bits) - 1);
    let mut exponent = ((v >> mantissa_bits) & ((1 << exp_bits) - 1)) as i32;
    let sign = v >> (exp_bits + mantissa_bits);
    exponent += min_exp;
    exponent += 127;
    let bits = (sign << 31) | ((exponent as u32) << 23) | (mantissa << (23 - mantissa_bits));
    f32::from_bits(bits)
}

fn rot_stride(f: Acf) -> usize {
    match f {
        Acf::None => 16,
        Acf::Float96NoW => 12,
        Acf::Fixed48NoW => 6,
        Acf::IntervalFixed32NoW | Acf::Fixed32NoW | Acf::Float32NoW => 4,
        Acf::Identity => 0,
    }
}

fn trans_stride(f: Acf) -> usize {
    match f {
        Acf::IntervalFixed32NoW => 4,
        Acf::Identity => 0,
        _ => 12,
    }
}

fn read_rot(b: &[u8], at: usize, f: Acf, mins: &[f32; 3], ranges: &[f32; 3]) -> Result<[f32; 4]> {
    Ok(match f {
        Acf::None => [rd_f32(b, at)?, rd_f32(b, at + 4)?, rd_f32(b, at + 8)?, rd_f32(b, at + 12)?],
        Acf::Float96NoW => w_from_xyz(rd_f32(b, at)?, rd_f32(b, at + 4)?, rd_f32(b, at + 8)?),
        Acf::Fixed48NoW => {
            let c = |v: u16| (v as i32 - 32767) as f32 / 32767.0;
            w_from_xyz(c(rd_u16(b, at)?), c(rd_u16(b, at + 2)?), c(rd_u16(b, at + 4)?))
        }
        Acf::Fixed32NoW => {
            let p = rd_u32(b, at)?;
            let x = ((p >> 21) as i32 - 1023) as f32 / 1023.0;
            let y = (((p & 0x001f_fc00) >> 10) as i32 - 1023) as f32 / 1023.0;
            let z = ((p & 0x3ff) as i32 - 511) as f32 / 511.0;
            w_from_xyz(x, y, z)
        }
        Acf::IntervalFixed32NoW => {
            let p = rd_u32(b, at)?;
            let x = ((p >> 21) as i32 - 1023) as f32 / 1023.0 * ranges[0] + mins[0];
            let y = (((p & 0x001f_fc00) >> 10) as i32 - 1023) as f32 / 1023.0 * ranges[1] + mins[1];
            let z = ((p & 0x3ff) as i32 - 511) as f32 / 511.0 * ranges[2] + mins[2];
            w_from_xyz(x, y, z)
        }
        Acf::Float32NoW => {
            let p = rd_u32(b, at)?;
            w_from_xyz(float_unpack(p >> 21, 7), float_unpack((p & 0x001f_fc00) >> 10, 7), float_unpack(p & 0x3ff, 6))
        }
        Acf::Identity => [0.0, 0.0, 0.0, 1.0],
    })
}

fn read_trans(b: &[u8], at: usize, f: Acf, mins: &[f32; 3], ranges: &[f32; 3]) -> Result<[f32; 3]> {
    Ok(match f {
        Acf::IntervalFixed32NoW => {
            let p = rd_u32(b, at)?;
            let x = ((p & 0x3ff) as i32 - 511) as f32 / 511.0 * ranges[0] + mins[0];
            let y = (((p & 0x001f_fc00) >> 10) as i32 - 1023) as f32 / 1023.0 * ranges[1] + mins[1];
            let z = ((p >> 21) as i32 - 1023) as f32 / 1023.0 * ranges[2] + mins[2];
            [x, y, z]
        }
        Acf::Identity => [0.0; 3],
        _ => [rd_f32(b, at)?, rd_f32(b, at + 4)?, rd_f32(b, at + 8)?],
    })
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

/// Keys of one half-track (translation or rotation): the values and their frames.
#[allow(clippy::too_many_arguments)]
fn read_keys<T>(stream: &[u8], offset: usize, n: usize, h: &Header, stride: usize, interval: bool, single: impl Fn(usize) -> Result<T>, key: impl Fn(usize, &[f32; 3], &[f32; 3]) -> Result<T>) -> Result<(Vec<T>, Vec<f32>)> {
    if n == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    if n == 1 {
        return Ok((vec![single(offset)?], vec![0.0]));
    }
    let mut at = offset;
    let (mut mins, mut ranges) = ([0f32; 3], [1f32; 3]);
    if interval {
        for c in 0..3 {
            mins[c] = rd_f32(stream, at + c * 4)?;
            ranges[c] = rd_f32(stream, at + 12 + c * 4)?;
        }
        at += 24;
    }
    let mut keys = Vec::with_capacity(n);
    for k in 0..n {
        keys.push(key(at + k * stride, &mins, &ranges)?);
    }
    at += n * stride;
    let frames = match h.key_format {
        KeyFormat::VariableKeyLerp => {
            let table = align4(at);
            let wide = h.num_frames > 0xff;
            (0..n).map(|k| if wide { rd_u16(stream, table + k * 2).map(|v| v as f32) } else { stream.get(table + k).map(|&v| v as f32).ok_or_else(|| Error::Format("animation stream too short".into())) }).collect::<Result<Vec<f32>>>()?
        }
        _ => {
            let last = h.num_frames.saturating_sub(1) as f32;
            (0..n).map(|k| k as f32 * last / (n - 1) as f32).collect()
        }
    };
    Ok((keys, frames))
}

/// Decode every track of a compressed stream.
pub fn decode(h: &Header, stream: &[u8]) -> Result<Anim> {
    if h.key_format == KeyFormat::PerTrackCompression {
        return Err(Error::Format("per-track compressed animations are not supported yet (no shipped sequence uses them)".into()));
    }
    let mut tracks = Vec::with_capacity(h.offsets.len() / 4);
    for t in h.offsets.chunks_exact(4) {
        let (to, tn, ro, rn) = (t[0].max(0) as usize, t[1].max(0) as usize, t[2].max(0) as usize, t[3].max(0) as usize);
        let tf = h.trans_format;
        let (trans, trans_frames) = read_keys(stream, to, tn, h, trans_stride(tf), tf == Acf::IntervalFixed32NoW, |at| read_trans(stream, at, Acf::None, &[0.0; 3], &[1.0; 3]), |at, m, r| read_trans(stream, at, tf, m, r))?;
        let rf = h.rot_format;
        let (rot, rot_frames) = read_keys(stream, ro, rn, h, rot_stride(rf), rf == Acf::IntervalFixed32NoW, |at| read_rot(stream, at, Acf::Float96NoW, &[0.0; 3], &[1.0; 3]), |at, m, r| read_rot(stream, at, rf, m, r))?;
        tracks.push(Track { trans, trans_frames, rot, rot_frames });
    }
    Ok(Anim { num_frames: h.num_frames, length: h.length, tracks })
}

fn pad(out: &mut Vec<u8>, with: u8) {
    while !out.len().is_multiple_of(4) {
        out.push(with);
    }
}

fn frame_table(out: &mut Vec<u8>, frames: &[f32], num_frames: usize) {
    pad(out, PAD);
    // Frames are written as given (a few shipped tables hold numbers past the
    // last frame; re-encoding keeps them), clamped only to the table's width.
    let max = if num_frames > 0xff { u16::MAX as f32 } else { u8::MAX as f32 };
    for &f in frames {
        let v = f.round().clamp(0.0, max) as u32;
        if num_frames > 0xff {
            out.extend_from_slice(&(v as u16).to_le_bytes());
        } else {
            out.push(v as u8);
        }
    }
    // The 10897 source pads frame tables with zeros; the game's cooker used
    // 0x55 like everywhere else (checked on every shipped sequence).
    pad(out, PAD);
}

/// Encode tracks as `AKF_VariableKeyLerp` with `ACF_Float96NoW` rotations and
/// `ACF_None` translations (what every shipped sequence uses): the stream and
/// `CompressedTrackOffsets`. Rotations are written as given (normalised, with
/// `w >= 0`, by [`normalize`]), so decoded keys re-encode to the same bytes.
pub fn encode(anim: &Anim) -> (Vec<u8>, Vec<i32>) {
    let mut out = Vec::new();
    let mut offsets = Vec::with_capacity(anim.tracks.len() * 4);
    for t in &anim.tracks {
        offsets.push(out.len() as i32);
        offsets.push(t.trans.len() as i32);
        for v in &t.trans {
            for c in v {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        if t.trans.len() > 1 {
            frame_table(&mut out, &t.trans_frames, anim.num_frames);
        }
        pad(&mut out, PAD);
        offsets.push(out.len() as i32);
        offsets.push(t.rot.len() as i32);
        for q in &t.rot {
            for c in &q[..3] {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        if t.rot.len() > 1 {
            frame_table(&mut out, &t.rot_frames, anim.num_frames);
        }
        pad(&mut out, PAD);
    }
    (out, offsets)
}

/// A unit quaternion with `w >= 0` (`FQuatFloat96NoW::FromQuat`).
pub fn normalize(q: [f32; 4]) -> [f32; 4] {
    let s = if q[3] < 0.0 { -1.0 } else { 1.0 };
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-12);
    [q[0] * s / l, q[1] * s / l, q[2] * s / l, q[3] * s / l]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// `FQuat` blend as the engine does it: the shorter way, normalised.
pub fn lerp_quat(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let s = if dot < 0.0 { -1.0 } else { 1.0 };
    let q = [a[0] + (b[0] * s - a[0]) * t, a[1] + (b[1] * s - a[1]) * t, a[2] + (b[2] * s - a[2]) * t, a[3] + (b[3] * s - a[3]) * t];
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-12);
    [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
}

fn sample_keys<T: Copy>(keys: &[T], frames: &[f32], f: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = *keys.first()?;
    if keys.len() == 1 || f <= frames[0] {
        return Some(first);
    }
    let last = keys.len() - 1;
    if f >= frames[last] {
        return Some(keys[last]);
    }
    let i = frames.partition_point(|&x| x <= f).saturating_sub(1).min(last - 1);
    let span = (frames[i + 1] - frames[i]).max(1e-6);
    Some(lerp(keys[i], keys[i + 1], (f - frames[i]) / span))
}

impl Track {
    /// The bone's local transform at frame `f` (`None` for a half-track without keys).
    pub fn sample(&self, f: f32) -> (Option<[f32; 4]>, Option<[f32; 3]>) {
        (sample_keys(&self.rot, &self.rot_frames, f, lerp_quat), sample_keys(&self.trans, &self.trans_frames, f, lerp3))
    }
}

/// Per-frame values of a track → the fewest keys that reproduce them within
/// the tolerances (a key is dropped when its neighbours interpolate it); a
/// constant track keeps one key.
pub fn reduce(rot: &[[f32; 4]], trans: &[[f32; 3]], rot_tol: f32, trans_tol: f32) -> Track {
    fn keep<T: Copy>(v: &[T], close: impl Fn(T, T, T, f32) -> bool) -> Vec<usize> {
        if v.len() <= 2 {
            return (0..v.len()).collect();
        }
        let mut kept = vec![0];
        let mut start = 0;
        let mut end = 2;
        while end < v.len() {
            let ok = (start + 1..end).all(|k| close(v[start], v[end], v[k], (k - start) as f32 / (end - start) as f32));
            if !ok {
                kept.push(end - 1);
                start = end - 1;
            }
            end += 1;
        }
        kept.push(v.len() - 1);
        kept
    }
    let qdiff = |a: [f32; 4], b: [f32; 4]| {
        let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs().min(1.0);
        2.0 * d.acos()
    };
    let vdiff = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    let rot: Vec<[f32; 4]> = rot.iter().map(|&q| normalize(q)).collect();
    let mut t = Track::default();
    if !rot.is_empty() {
        if rot.iter().all(|&q| qdiff(q, rot[0]) <= rot_tol) {
            t.rot = vec![rot[0]];
            t.rot_frames = vec![0.0];
        } else {
            let k = keep(&rot, |a, b, x, s| qdiff(lerp_quat(a, b, s), x) <= rot_tol);
            t.rot = k.iter().map(|&i| rot[i]).collect();
            t.rot_frames = k.iter().map(|&i| i as f32).collect();
        }
    }
    if !trans.is_empty() {
        if trans.iter().all(|&v| vdiff(v, trans[0]) <= trans_tol) {
            t.trans = vec![trans[0]];
            t.trans_frames = vec![0.0];
        } else {
            let k = keep(trans, |a, b, x, s| vdiff(lerp3(a, b, s), x) <= trans_tol);
            t.trans = k.iter().map(|&i| trans[i]).collect();
            t.trans_frames = k.iter().map(|&i| i as f32).collect();
        }
    }
    t
}

/// Rotation tolerance of [`reduce`] (radians) and translation tolerance (cm):
/// far below what can be seen, close to what the shipped sequences keep.
pub const ROT_TOLERANCE: f32 = 0.0005;
pub const TRANS_TOLERANCE: f32 = 0.01;

/// The header of a parsed `AnimSequence` (its properties; absent ones take
/// the class defaults).
pub fn header(pkg: &tempest_upk::Package, o: &crate::object::Object) -> Header {
    use crate::value::Value;
    let get = |name: &str| o.property(name, pkg).map(|p| &p.value);
    let enum_name = |name: &str| match get(name) {
        Some(Value::Enum(n)) | Some(Value::Name(n)) => Some(pkg.fname_to_string(*n)),
        _ => None,
    };
    let num_frames = match get("NumFrames") {
        Some(Value::Int(n)) => (*n).max(0) as usize,
        _ => 0,
    };
    let length = match get("SequenceLength") {
        Some(Value::Float(f)) => *f,
        _ => 0.0,
    };
    let offsets = match get("CompressedTrackOffsets") {
        Some(Value::Array(a)) => a.iter().map(|v| if let Value::Int(i) = v { *i } else { 0 }).collect(),
        _ => Vec::new(),
    };
    Header {
        num_frames,
        length,
        key_format: KeyFormat::parse(enum_name("KeyEncodingFormat").as_deref()),
        rot_format: Acf::parse(enum_name("RotationCompressionFormat").as_deref()),
        trans_format: Acf::parse(enum_name("TranslationCompressionFormat").as_deref()),
        offsets,
    }
}

/// The compressed stream of a parsed `AnimSequence`.
pub fn stream(o: &crate::object::Object) -> Option<&[u8]> {
    match &o.native {
        crate::object::Native::AnimSequence(a) => Some(&a.compressed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode() {
        let rot: Vec<[f32; 4]> = (0..40).map(|i| normalize([0.0, 0.0, (i as f32 * 0.05).sin(), (i as f32 * 0.05).cos()])).collect();
        let trans: Vec<[f32; 3]> = (0..40).map(|i| [i as f32, 0.0, 0.0]).collect();
        let still = vec![[0.0, 0.0, 0.0, 1.0]; 40];
        let a = Anim { num_frames: 40, length: 39.0 / 30.0, tracks: vec![reduce(&rot, &trans, ROT_TOLERANCE, TRANS_TOLERANCE), reduce(&still, &[[1.0, 2.0, 3.0]; 40], ROT_TOLERANCE, TRANS_TOLERANCE)] };
        // A straight line is two keys; a constant track one.
        assert_eq!(a.tracks[0].trans.len(), 2);
        assert_eq!(a.tracks[1].rot.len(), 1);
        let (stream, offsets) = encode(&a);
        let h = Header { num_frames: 40, length: a.length, key_format: KeyFormat::VariableKeyLerp, rot_format: Acf::Float96NoW, trans_format: Acf::None, offsets };
        let d = decode(&h, &stream).unwrap();
        // x, y, z are stored as they are; w is rebuilt from them.
        for (dt, at) in d.tracks.iter().zip(&a.tracks) {
            assert_eq!(dt.trans, at.trans);
            assert_eq!((&dt.trans_frames, &dt.rot_frames), (&at.trans_frames, &at.rot_frames));
            for (x, y) in dt.rot.iter().zip(&at.rot) {
                assert_eq!(x[..3], y[..3]);
                assert!((x[3] - y[3]).abs() < 1e-5);
            }
        }
        // Every frame comes back within the tolerance.
        for (f, q) in rot.iter().enumerate() {
            let (r, _) = d.tracks[0].sample(f as f32);
            let r = r.unwrap();
            let dot = (r[0] * q[0] + r[1] * q[1] + r[2] * q[2] + r[3] * q[3]).abs().min(1.0);
            assert!(2.0 * dot.acos() <= ROT_TOLERANCE * 2.0);
        }
        // And re-encodes to the same bytes.
        assert_eq!(encode(&d).0, stream);
    }

    #[test]
    fn float_packer_decodes() {
        assert_eq!(float_unpack(0, 7), 0.0);
        // 1.0 in 3e7: exponent 0 → biased 4 (min -4), mantissa 0.
        assert_eq!(float_unpack(4 << 7, 7), 1.0);
    }
}
