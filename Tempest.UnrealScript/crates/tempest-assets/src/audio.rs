//! PCM audio in and out: WAV reading, Ogg Vorbis encoding and decoding
//! (libvorbis, aoTuV flavour, through `vorbis_rs`), and the edits the
//! Studio's sound editor applies.

use std::num::{NonZeroU32, NonZeroU8};

use tempest_upk::io::{Error, Result};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisDecoder, VorbisEncoderBuilder};

/// Planar float PCM.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    /// One Vec per channel, samples in -1..1.
    pub channels: Vec<Vec<f32>>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / self.sample_rate as f64 }
    }

    /// From interleaved 16-bit little-endian samples.
    pub fn from_interleaved_i16(bytes: &[u8], channels: usize, sample_rate: u32) -> Pcm {
        let n = bytes.len() / 2 / channels.max(1);
        let mut out = vec![Vec::with_capacity(n); channels];
        for f in 0..n {
            for (c, ch) in out.iter_mut().enumerate() {
                let i = (f * channels + c) * 2;
                ch.push(i16::from_le_bytes([bytes[i], bytes[i + 1]]) as f32 / 32768.0);
            }
        }
        Pcm { sample_rate, channels: out }
    }

    /// Interleaved 16-bit little-endian samples.
    pub fn to_interleaved_i16(&self) -> Vec<u8> {
        let n = self.frames();
        let mut out = Vec::with_capacity(n * self.channels.len() * 2);
        for f in 0..n {
            for ch in &self.channels {
                let v = (ch[f].clamp(-1.0, 1.0) * 32767.0).round() as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }
}

/// Decode an Ogg Vorbis file.
pub fn decode_ogg(bytes: &[u8]) -> Result<Pcm> {
    let mut dec = VorbisDecoder::new(std::io::Cursor::new(bytes)).map_err(|e| Error::Format(format!("Ogg Vorbis: {e}")))?;
    let channels = dec.channels().get() as usize;
    let mut pcm = Pcm { sample_rate: dec.sampling_frequency().get(), channels: vec![Vec::new(); channels] };
    while let Some(block) = dec.decode_audio_block().map_err(|e| Error::Format(format!("Ogg Vorbis: {e}")))? {
        for (c, samples) in block.samples().iter().enumerate() {
            pcm.channels[c].extend_from_slice(samples);
        }
    }
    Ok(pcm)
}

/// Encode to Ogg Vorbis at a VBR quality in -0.1..1 (0.4 ≈ 128 kb/s
/// stereo, what the game's own streams use).
pub fn encode_ogg(pcm: &Pcm, quality: f32) -> Result<Vec<u8>> {
    let channels = NonZeroU8::new(pcm.channels.len() as u8).ok_or_else(|| Error::Format("no audio channels".into()))?;
    let rate = NonZeroU32::new(pcm.sample_rate).ok_or_else(|| Error::Format("sample rate is zero".into()))?;
    let mut out = Vec::new();
    {
        let mut enc = VorbisEncoderBuilder::new(rate, channels, &mut out)
            .map_err(|e| Error::Format(format!("Vorbis encoder: {e}")))?
            .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr { target_quality: quality.clamp(-0.1, 1.0) })
            .build()
            .map_err(|e| Error::Format(format!("Vorbis encoder: {e}")))?;
        let slices: Vec<&[f32]> = pcm.channels.iter().map(|c| c.as_slice()).collect();
        // Blocks of 64k frames keep the encoder's buffers small.
        let n = pcm.frames();
        let mut at = 0;
        while at < n {
            let end = (at + 65536).min(n);
            let block: Vec<&[f32]> = slices.iter().map(|s| &s[at..end]).collect();
            enc.encode_audio_block(block).map_err(|e| Error::Format(format!("Vorbis encoder: {e}")))?;
            at = end;
        }
        enc.finish().map_err(|e| Error::Format(format!("Vorbis encoder: {e}")))?;
    }
    Ok(out)
}

/// Read a RIFF WAVE file: 8/16/24/32-bit integer PCM or 32-bit float.
pub fn decode_wav(bytes: &[u8]) -> Result<Pcm> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Format("not a WAV file".into()));
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = bytes.get(pos + 8..(pos + 8 + size).min(bytes.len())).unwrap_or(&[]);
        if id == b"fmt " && body.len() >= 16 {
            let mut tag = u16::from_le_bytes([body[0], body[1]]);
            if tag == 0xFFFE && body.len() >= 26 {
                tag = u16::from_le_bytes([body[24], body[25]]);
            }
            fmt = Some((tag, u16::from_le_bytes([body[2], body[3]]), u32::from_le_bytes(body[4..8].try_into().unwrap()), u16::from_le_bytes([body[14], body[15]])));
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + size + (size & 1);
    }
    let (tag, channels, rate, bits) = fmt.ok_or_else(|| Error::Format("WAV: no fmt chunk".into()))?;
    let data = data.ok_or_else(|| Error::Format("WAV: no data chunk".into()))?;
    let channels = channels.max(1) as usize;
    let bytes_per = (bits as usize).div_ceil(8).max(1);
    let n = data.len() / (bytes_per * channels);
    let mut out = vec![Vec::with_capacity(n); channels];
    for f in 0..n {
        for (c, ch) in out.iter_mut().enumerate() {
            let i = (f * channels + c) * bytes_per;
            let s = &data[i..i + bytes_per];
            let v = match (tag, bits) {
                (1, 8) => (s[0] as f32 - 128.0) / 128.0,
                (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
                (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                _ => return Err(Error::Format(format!("WAV: unsupported format (tag {tag}, {bits} bits)"))),
            };
            ch.push(v);
        }
    }
    Ok(Pcm { sample_rate: rate, channels: out })
}

/// A 16-bit PCM WAV file.
pub fn encode_wav(pcm: &Pcm) -> Vec<u8> {
    let data = pcm.to_interleaved_i16();
    let channels = pcm.channels.len() as u16;
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&pcm.sample_rate.to_le_bytes());
    out.extend_from_slice(&(pcm.sample_rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// Decode an audio file by its contents: Ogg Vorbis, WAV, or a Wwise
/// Vorbis `.wem`.
pub fn decode_any(bytes: &[u8]) -> Result<Pcm> {
    if bytes.starts_with(b"OggS") {
        decode_ogg(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 22 {
        let tag = u16::from_le_bytes([bytes[20], bytes[21]]);
        if tag == 0xFFFF {
            let (ogg, _) = crate::wwise::vorbis::wem_to_ogg(bytes)?;
            decode_ogg(&ogg)
        } else {
            decode_wav(bytes)
        }
    } else {
        Err(Error::Format("unsupported audio file (use .ogg, .wav or .wem)".into()))
    }
}

/// The edits the Studio's sound editor applies before re-encoding; times
/// in seconds.
#[derive(Clone, Debug, Default)]
pub struct Edits {
    /// Keep only this range.
    pub trim: Option<(f64, f64)>,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Linear gain multiplier (1 = unchanged).
    pub gain: f32,
    /// Scale the peak to this level (0 = off).
    pub normalize: f32,
    /// Make mono or stereo (0 = keep).
    pub channels: usize,
}

pub fn apply_edits(pcm: &Pcm, e: &Edits) -> Pcm {
    let rate = pcm.sample_rate as f64;
    let mut out = pcm.clone();
    if let Some((start, end)) = e.trim {
        let n = out.frames();
        let a = ((start.max(0.0) * rate).round() as usize).min(n);
        let b = ((end * rate).round() as usize).clamp(a, n);
        for ch in out.channels.iter_mut() {
            *ch = ch[a..b].to_vec();
        }
    }
    let n = out.frames();
    if e.fade_in > 0.0 {
        let len = ((e.fade_in * rate) as usize).min(n).max(1);
        for ch in out.channels.iter_mut() {
            for (i, s) in ch.iter_mut().take(len).enumerate() {
                *s *= i as f32 / len as f32;
            }
        }
    }
    if e.fade_out > 0.0 {
        let len = ((e.fade_out * rate) as usize).min(n).max(1);
        for ch in out.channels.iter_mut() {
            for i in 0..len {
                let k = n - len + i;
                ch[k] *= 1.0 - (i as f32 + 1.0) / len as f32;
            }
        }
    }
    let mut gain = if e.gain > 0.0 { e.gain } else { 1.0 };
    if e.normalize > 0.0 {
        let peak = out.channels.iter().flat_map(|c| c.iter()).fold(0f32, |m, s| m.max(s.abs()));
        if peak > 1e-6 {
            gain *= e.normalize / peak;
        }
    }
    if (gain - 1.0).abs() > 1e-6 {
        for ch in out.channels.iter_mut() {
            for s in ch.iter_mut() {
                *s = (*s * gain).clamp(-1.0, 1.0);
            }
        }
    }
    match (e.channels, out.channels.len()) {
        (1, c) if c > 1 => {
            let mut mono = vec![0f32; n];
            for ch in &out.channels {
                for (m, s) in mono.iter_mut().zip(ch) {
                    *m += *s / c as f32;
                }
            }
            out.channels = vec![mono];
        }
        (2, 1) => {
            let m = out.channels[0].clone();
            out.channels.push(m);
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f32, channels: usize) -> Pcm {
        let n = (rate as f32 * secs) as usize;
        let ch: Vec<f32> = (0..n).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
        Pcm { sample_rate: rate, channels: vec![ch; channels] }
    }

    #[test]
    fn ogg_round_trip() {
        let pcm = tone(44100, 0.5, 2);
        let ogg = encode_ogg(&pcm, 0.4).unwrap();
        assert!(ogg.starts_with(b"OggS"));
        let back = decode_ogg(&ogg).unwrap();
        assert_eq!(back.sample_rate, 44100);
        assert_eq!(back.channels.len(), 2);
        assert!((back.frames() as i64 - pcm.frames() as i64).abs() < 64, "{} vs {}", back.frames(), pcm.frames());
        assert_eq!(decode_any(&ogg).unwrap().channels.len(), 2);
    }

    #[test]
    fn wav_round_trip() {
        let pcm = tone(48000, 0.1, 1);
        let wav = encode_wav(&pcm);
        let back = decode_wav(&wav).unwrap();
        assert_eq!(back.sample_rate, 48000);
        assert_eq!(back.frames(), pcm.frames());
        assert!((back.channels[0][100] - pcm.channels[0][100]).abs() < 1e-3);
    }

    #[test]
    fn edits_trim_fade_normalize() {
        let pcm = tone(1000, 2.0, 1);
        let e = Edits { trim: Some((0.5, 1.5)), fade_in: 0.1, fade_out: 0.1, gain: 1.0, normalize: 1.0, channels: 2 };
        let out = apply_edits(&pcm, &e);
        assert_eq!(out.frames(), 1000);
        assert_eq!(out.channels.len(), 2);
        assert!(out.channels[0][0].abs() < 1e-6);
        let peak = out.channels[0].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 1.0).abs() < 1e-3, "{peak}");
    }
}
