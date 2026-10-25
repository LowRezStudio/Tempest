//! Pixel formats (`EPixelFormat`), DDS files and block decompression.

use tempest_upk::io::{Error, Result};

/// The pixel formats the game's textures use, by `EPixelFormat` tag name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Dxt1,
    Dxt3,
    Dxt5,
    Bc5,
    A8R8G8B8,
    G8,
    V8U8,
    G16R16,
    A16B16G16R16,
    FloatRgba,
    R16F,
    R32F,
}

impl PixelFormat {
    pub fn from_name(name: &str) -> Option<PixelFormat> {
        Some(match name.to_ascii_lowercase().as_str() {
            "pf_dxt1" => PixelFormat::Dxt1,
            "pf_dxt3" => PixelFormat::Dxt3,
            "pf_dxt5" => PixelFormat::Dxt5,
            "pf_bc5" => PixelFormat::Bc5,
            "pf_a8r8g8b8" => PixelFormat::A8R8G8B8,
            "pf_g8" => PixelFormat::G8,
            "pf_v8u8" => PixelFormat::V8U8,
            "pf_g16r16" => PixelFormat::G16R16,
            "pf_a16b16g16r16" => PixelFormat::A16B16G16R16,
            "pf_floatrgba" => PixelFormat::FloatRgba,
            "pf_r16f" => PixelFormat::R16F,
            "pf_r32f" => PixelFormat::R32F,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            PixelFormat::Dxt1 => "PF_DXT1",
            PixelFormat::Dxt3 => "PF_DXT3",
            PixelFormat::Dxt5 => "PF_DXT5",
            PixelFormat::Bc5 => "PF_BC5",
            PixelFormat::A8R8G8B8 => "PF_A8R8G8B8",
            PixelFormat::G8 => "PF_G8",
            PixelFormat::V8U8 => "PF_V8U8",
            PixelFormat::G16R16 => "PF_G16R16",
            PixelFormat::A16B16G16R16 => "PF_A16B16G16R16",
            PixelFormat::FloatRgba => "PF_FloatRGBA",
            PixelFormat::R16F => "PF_R16F",
            PixelFormat::R32F => "PF_R32F",
        }
    }

    pub fn is_block_compressed(self) -> bool {
        matches!(self, PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5 | PixelFormat::Bc5)
    }

    /// Bytes per 4x4 block (compressed) or per pixel.
    pub fn block_bytes(self) -> usize {
        match self {
            PixelFormat::Dxt1 => 8,
            PixelFormat::Dxt3 | PixelFormat::Dxt5 | PixelFormat::Bc5 => 16,
            PixelFormat::A8R8G8B8 | PixelFormat::G16R16 | PixelFormat::R32F => 4,
            PixelFormat::G8 => 1,
            PixelFormat::V8U8 | PixelFormat::R16F => 2,
            PixelFormat::A16B16G16R16 | PixelFormat::FloatRgba => 8,
        }
    }

    /// Size in bytes of one mip level (`CalculateImageBytes`).
    pub fn image_bytes(self, w: u32, h: u32) -> usize {
        if self.is_block_compressed() {
            (w.max(1).div_ceil(4) as usize) * (h.max(1).div_ceil(4) as usize) * self.block_bytes()
        } else {
            w.max(1) as usize * h.max(1) as usize * self.block_bytes()
        }
    }
}

/// One mip level ready to be written.
pub struct DdsMip<'a> {
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
}

const DDSD_CAPS: u32 = 0x1;
const DDSD_HEIGHT: u32 = 0x2;
const DDSD_WIDTH: u32 = 0x4;
const DDSD_PITCH: u32 = 0x8;
const DDSD_PIXELFORMAT: u32 = 0x1000;
const DDSD_MIPMAPCOUNT: u32 = 0x20000;
const DDSD_LINEARSIZE: u32 = 0x80000;
const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_FOURCC: u32 = 0x4;
const DDPF_RGB: u32 = 0x40;
const DDPF_LUMINANCE: u32 = 0x20000;
const DDPF_BUMPDUDV: u32 = 0x80000;
const DDSCAPS_COMPLEX: u32 = 0x8;
const DDSCAPS_TEXTURE: u32 = 0x1000;
const DDSCAPS_MIPMAP: u32 = 0x400000;

fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

/// Build a DDS file from mips of one format (mip 0 first).
pub fn write_dds(format: PixelFormat, mips: &[DdsMip]) -> Result<Vec<u8>> {
    let Some(top) = mips.first() else { return Err(Error::Format("no mips".into())) };
    let mut h = vec![0u8; 128];
    let put = |h: &mut Vec<u8>, at: usize, v: u32| h[at..at + 4].copy_from_slice(&v.to_le_bytes());
    h[0..4].copy_from_slice(b"DDS ");
    put(&mut h, 4, 124);
    let mut flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
    if mips.len() > 1 {
        flags |= DDSD_MIPMAPCOUNT;
    }
    // Pixel format block at 76: size 32, flags, fourcc, bitcount, masks.
    put(&mut h, 76, 32);
    let (pf_flags, cc, bits, masks): (u32, u32, u32, [u32; 4]) = match format {
        PixelFormat::Dxt1 => (DDPF_FOURCC, fourcc(b"DXT1"), 0, [0; 4]),
        PixelFormat::Dxt3 => (DDPF_FOURCC, fourcc(b"DXT3"), 0, [0; 4]),
        PixelFormat::Dxt5 => (DDPF_FOURCC, fourcc(b"DXT5"), 0, [0; 4]),
        PixelFormat::Bc5 => (DDPF_FOURCC, fourcc(b"ATI2"), 0, [0; 4]),
        PixelFormat::A8R8G8B8 => (DDPF_RGB | DDPF_ALPHAPIXELS, 0, 32, [0x00ff0000, 0x0000ff00, 0x000000ff, 0xff000000]),
        PixelFormat::G8 => (DDPF_LUMINANCE, 0, 8, [0xff, 0, 0, 0]),
        PixelFormat::V8U8 => (DDPF_BUMPDUDV, 0, 16, [0x00ff, 0xff00, 0, 0]),
        PixelFormat::G16R16 => (DDPF_RGB, 0, 32, [0x0000ffff, 0xffff0000, 0, 0]),
        PixelFormat::A16B16G16R16 => (DDPF_FOURCC, 36, 0, [0; 4]),
        PixelFormat::FloatRgba => (DDPF_FOURCC, 113, 0, [0; 4]),
        PixelFormat::R16F => (DDPF_FOURCC, 111, 0, [0; 4]),
        PixelFormat::R32F => (DDPF_FOURCC, 114, 0, [0; 4]),
    };
    if format.is_block_compressed() {
        flags |= DDSD_LINEARSIZE;
        put(&mut h, 20, format.image_bytes(top.width, top.height) as u32);
    } else {
        flags |= DDSD_PITCH;
        put(&mut h, 20, top.width * format.block_bytes() as u32);
    }
    put(&mut h, 8, flags);
    put(&mut h, 12, top.height);
    put(&mut h, 16, top.width);
    put(&mut h, 28, mips.len() as u32);
    put(&mut h, 80, pf_flags);
    put(&mut h, 84, cc);
    put(&mut h, 88, bits);
    for (i, m) in masks.iter().enumerate() {
        put(&mut h, 92 + 4 * i, *m);
    }
    let mut caps = DDSCAPS_TEXTURE;
    if mips.len() > 1 {
        caps |= DDSCAPS_COMPLEX | DDSCAPS_MIPMAP;
    }
    put(&mut h, 108, caps);
    let mut out = h;
    for m in mips {
        let want = format.image_bytes(m.width, m.height);
        if m.data.len() != want {
            return Err(Error::Format(format!("mip {}x{}: {} bytes, expected {want} for {}", m.width, m.height, m.data.len(), format.name())));
        }
        out.extend_from_slice(m.data);
    }
    Ok(out)
}

/// A parsed DDS: format and mips (mip 0 first).
pub struct Dds {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub mips: Vec<Vec<u8>>,
}

pub fn read_dds(bytes: &[u8]) -> Result<Dds> {
    if bytes.len() < 128 || &bytes[0..4] != b"DDS " {
        return Err(Error::Format("not a DDS file".into()));
    }
    let get = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let height = get(12);
    let width = get(16);
    let flags = get(8);
    let mip_count = if flags & DDSD_MIPMAPCOUNT != 0 { get(28).max(1) } else { 1 };
    let pf_flags = get(80);
    let cc = get(84);
    let bits = get(88);
    let masks = [get(92), get(96), get(100), get(104)];
    let mut data_start = 128;
    let format = if pf_flags & DDPF_FOURCC != 0 {
        if cc == fourcc(b"DX10") {
            data_start = 148;
            let dxgi = get(128);
            match dxgi {
                71 | 72 => PixelFormat::Dxt1,
                74 | 75 => PixelFormat::Dxt3,
                77 | 78 => PixelFormat::Dxt5,
                83 => PixelFormat::Bc5,
                87 | 91 => PixelFormat::A8R8G8B8,
                61 => PixelFormat::G8,
                10 => PixelFormat::FloatRgba,
                54 => PixelFormat::R16F,
                41 => PixelFormat::R32F,
                _ => return Err(Error::Format(format!("unsupported DXGI format {dxgi}"))),
            }
        } else if cc == fourcc(b"DXT1") {
            PixelFormat::Dxt1
        } else if cc == fourcc(b"DXT3") {
            PixelFormat::Dxt3
        } else if cc == fourcc(b"DXT5") {
            PixelFormat::Dxt5
        } else if cc == fourcc(b"ATI2") || cc == fourcc(b"BC5U") {
            PixelFormat::Bc5
        } else if cc == 36 {
            PixelFormat::A16B16G16R16
        } else if cc == 113 {
            PixelFormat::FloatRgba
        } else if cc == 111 {
            PixelFormat::R16F
        } else if cc == 114 {
            PixelFormat::R32F
        } else {
            return Err(Error::Format(format!("unsupported DDS fourcc {:#x}", cc)));
        }
    } else if pf_flags & DDPF_RGB != 0 && bits == 32 && masks[0] == 0x00ff0000 && masks[1] == 0x0000ff00 && masks[2] == 0xff {
        PixelFormat::A8R8G8B8
    } else if pf_flags & DDPF_RGB != 0 && bits == 32 && masks[0] == 0xffff && masks[1] == 0xffff0000 {
        PixelFormat::G16R16
    } else if pf_flags & DDPF_LUMINANCE != 0 && bits == 8 {
        PixelFormat::G8
    } else if pf_flags & DDPF_BUMPDUDV != 0 && bits == 16 {
        PixelFormat::V8U8
    } else {
        return Err(Error::Format(format!("unsupported DDS pixel format (flags {pf_flags:#x}, {bits} bpp)")));
    };
    let mut mips = Vec::new();
    let mut pos = data_start;
    let (mut w, mut h) = (width, height);
    for _ in 0..mip_count {
        let n = format.image_bytes(w, h);
        let d = bytes.get(pos..pos + n).ok_or(Error::UnexpectedEof { at: pos, wanted: n, len: bytes.len() })?;
        mips.push(d.to_vec());
        pos += n;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    Ok(Dds { format, width, height, mips })
}

// ----------------------------------------------------------------------------
// Decoding to RGBA8
// ----------------------------------------------------------------------------

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

fn decode_bc1_block(block: &[u8], out: &mut [[u8; 4]; 16], opaque: bool) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let p0 = rgb565(c0);
    let p1 = rgb565(c1);
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || opaque {
        for i in 0..3 {
            pal[2][i] = ((2 * p0[i] as u32 + p1[i] as u32) / 3) as u8;
            pal[3][i] = ((p0[i] as u32 + 2 * p1[i] as u32) / 3) as u8;
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for i in 0..3 {
            pal[2][i] = ((p0[i] as u32 + p1[i] as u32) / 2) as u8;
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

fn decode_alpha_block(block: &[u8], out: &mut [u8; 16]) {
    let a0 = block[0] as u32;
    let a1 = block[1] as u32;
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7u32 {
            pal[i as usize + 1] = (((7 - i) * a0 + i * a1) / 7) as u8;
        }
    } else {
        for i in 1..5u32 {
            pal[i as usize + 1] = (((5 - i) * a0 + i * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits: u64 = 0;
    for i in 0..6 {
        bits |= (block[2 + i] as u64) << (8 * i);
    }
    for (i, a) in out.iter_mut().enumerate() {
        *a = pal[((bits >> (3 * i)) & 7) as usize];
    }
}

/// Decode one mip level to tightly packed RGBA8.
pub fn decode_rgba(format: PixelFormat, w: u32, h: u32, data: &[u8]) -> Result<Vec<u8>> {
    let (w, h) = (w.max(1) as usize, h.max(1) as usize);
    let want = format.image_bytes(w as u32, h as u32);
    if data.len() < want {
        return Err(Error::Format(format!("{} bytes of pixel data, need {want}", data.len())));
    }
    let mut out = vec![0u8; w * h * 4];
    match format {
        PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5 | PixelFormat::Bc5 => {
            let bw = w.div_ceil(4);
            let bh = h.div_ceil(4);
            let bs = format.block_bytes();
            for by in 0..bh {
                for bx in 0..bw {
                    let block = &data[(by * bw + bx) * bs..][..bs];
                    let mut px = [[0u8; 4]; 16];
                    match format {
                        PixelFormat::Dxt1 => decode_bc1_block(block, &mut px, false),
                        PixelFormat::Dxt3 => {
                            decode_bc1_block(&block[8..], &mut px, true);
                            for (i, p) in px.iter_mut().enumerate() {
                                let a = (block[i / 2] >> (4 * (i % 2))) & 15;
                                p[3] = a * 17;
                            }
                        }
                        PixelFormat::Dxt5 => {
                            decode_bc1_block(&block[8..], &mut px, true);
                            let mut alpha = [0u8; 16];
                            decode_alpha_block(block, &mut alpha);
                            for (p, a) in px.iter_mut().zip(alpha) {
                                p[3] = a;
                            }
                        }
                        PixelFormat::Bc5 => {
                            let mut r = [0u8; 16];
                            let mut g = [0u8; 16];
                            decode_alpha_block(block, &mut r);
                            decode_alpha_block(&block[8..], &mut g);
                            for (i, p) in px.iter_mut().enumerate() {
                                *p = [r[i], g[i], 0, 255];
                            }
                        }
                        _ => unreachable!(),
                    }
                    for (i, p) in px.iter().enumerate() {
                        let x = bx * 4 + i % 4;
                        let y = by * 4 + i / 4;
                        if x < w && y < h {
                            out[(y * w + x) * 4..][..4].copy_from_slice(p);
                        }
                    }
                }
            }
        }
        PixelFormat::A8R8G8B8 => {
            for i in 0..w * h {
                let p = &data[i * 4..][..4];
                out[i * 4..][..4].copy_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
        PixelFormat::G8 => {
            for i in 0..w * h {
                let v = data[i];
                out[i * 4..][..4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        PixelFormat::V8U8 => {
            for i in 0..w * h {
                let u = (data[i * 2] as i8 as i16 + 128) as u8;
                let v = (data[i * 2 + 1] as i8 as i16 + 128) as u8;
                out[i * 4..][..4].copy_from_slice(&[u, v, 255, 255]);
            }
        }
        PixelFormat::G16R16 => {
            for i in 0..w * h {
                out[i * 4..][..4].copy_from_slice(&[data[i * 4 + 1], data[i * 4 + 3], 0, 255]);
            }
        }
        PixelFormat::A16B16G16R16 => {
            for i in 0..w * h {
                let p = &data[i * 8..][..8];
                out[i * 4..][..4].copy_from_slice(&[p[1], p[3], p[5], p[7]]);
            }
        }
        PixelFormat::FloatRgba => {
            for i in 0..w * h {
                let p = &data[i * 8..][..8];
                let f = |j: usize| (half_to_f32(u16::from_le_bytes([p[j * 2], p[j * 2 + 1]])).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4..][..4].copy_from_slice(&[f(0), f(1), f(2), f(3)]);
            }
        }
        PixelFormat::R16F => {
            for i in 0..w * h {
                let v = (half_to_f32(u16::from_le_bytes([data[i * 2], data[i * 2 + 1]])).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4..][..4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        PixelFormat::R32F => {
            for i in 0..w * h {
                let v = (f32::from_le_bytes([data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]]).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4..][..4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }
    Ok(out)
}

pub fn half_to_f32(h: u16) -> f32 {
    let s = ((h >> 15) & 1) as u32;
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as u32;
    let bits = if e == 0 {
        if m == 0 {
            s << 31
        } else {
            // Subnormal: normalise.
            let mut e2 = -1;
            let mut m2 = m;
            while m2 & 0x400 == 0 {
                m2 <<= 1;
                e2 -= 1;
            }
            // m2 = 1.f × 2^10 after k shifts (e2 = -1 - k): value = 1.f × 2^(-14 - k).
            (s << 31) | (((e2 + 2 + 127 - 15) as u32) << 23) | ((m2 & 0x3ff) << 13)
        }
    } else if e == 31 {
        (s << 31) | 0x7f80_0000 | (m << 13)
    } else {
        (s << 31) | (((e + 127 - 15) as u32) << 23) | (m << 13)
    };
    f32::from_bits(bits)
}

/// Encode RGBA8 pixels as a PNG file.
pub fn write_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    write_png_with(w, h, rgba, false)
}

/// `fast`: fastest deflate level (bigger file), for previews.
pub fn write_png_with(w: u32, h: u32, rgba: &[u8], fast: bool) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if fast {
            // Fast deflate, adaptive filtering kept: the filter is what makes
            // texture PNGs small, the deflate level barely matters.
            enc.set_compression(png::Compression::Fast);
        }
        let mut writer = enc.write_header().map_err(|e| Error::Format(e.to_string()))?;
        writer.write_image_data(rgba).map_err(|e| Error::Format(e.to_string()))?;
    }
    Ok(out)
}

/// Decode a PNG file to RGBA8.
pub fn read_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = dec.read_info().map_err(|e| Error::Format(e.to_string()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| Error::Format(e.to_string()))?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let src = &buf[..info.buffer_size()];
    let rgba = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => src.to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) => {
            let mut v = Vec::with_capacity(n * 4);
            for p in src.chunks(3) {
                v.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
            v
        }
        (png::ColorType::Grayscale, png::BitDepth::Eight) => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => src.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        (ct, bd) => return Err(Error::Format(format!("unsupported PNG layout {ct:?} {bd:?} (use 8-bit RGB/RGBA/grey)"))),
    };
    Ok((w, h, rgba))
}

// ----------------------------------------------------------------------------
// Encoding from RGBA8
// ----------------------------------------------------------------------------

/// Halve an RGBA8 image (box filter); odd sizes drop the last row/column
/// like the engine's `GenerateMipMap` for non-power-of-two edges.
pub fn downsample_rgba(w: u32, h: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for dy in 0..2 {
                for dx in 0..2 {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    let p = &rgba[((sy * w + sx) * 4) as usize..][..4];
                    for c in 0..4 {
                        acc[c] += p[c] as u32;
                    }
                    n += 1;
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// Full mip chain of an RGBA8 image, down to 1x1.
pub fn rgba_mip_chain(w: u32, h: u32, rgba: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
    let mut chain = vec![(w, h, rgba.to_vec())];
    while let Some((cw, ch, data)) = chain.last() {
        if *cw <= 1 && *ch <= 1 {
            break;
        }
        chain.push(downsample_rgba(*cw, *ch, data));
    }
    chain
}

/// Encode RGBA8 pixels into `format`. Block formats need at least 4x4:
/// smaller images are padded by edge replication, as the engine stores a
/// full block for its smallest mips.
pub fn encode_rgba(format: PixelFormat, w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let (w, h) = (w.max(1), h.max(1));
    if rgba.len() < (w * h * 4) as usize {
        return Err(Error::Format("pixel buffer too small".into()));
    }
    if format.is_block_compressed() {
        let (pw, ph) = (w.max(4), h.max(4));
        let src: std::borrow::Cow<[u8]> = if (pw, ph) != (w, h) {
            let mut padded = vec![0u8; (pw * ph * 4) as usize];
            for y in 0..ph {
                for x in 0..pw {
                    let sx = x.min(w - 1);
                    let sy = y.min(h - 1);
                    padded[((y * pw + x) * 4) as usize..][..4].copy_from_slice(&rgba[((sy * w + sx) * 4) as usize..][..4]);
                }
            }
            padded.into()
        } else {
            rgba.into()
        };
        let tf = match format {
            PixelFormat::Dxt1 => texpresso::Format::Bc1,
            PixelFormat::Dxt3 => texpresso::Format::Bc2,
            PixelFormat::Dxt5 => texpresso::Format::Bc3,
            PixelFormat::Bc5 => texpresso::Format::Bc5,
            _ => unreachable!(),
        };
        let mut out = vec![0u8; tf.compressed_size(pw as usize, ph as usize)];
        let params = texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, weights: texpresso::COLOUR_WEIGHTS_PERCEPTUAL, weigh_colour_by_alpha: format == PixelFormat::Dxt5 };
        tf.compress(&src, pw as usize, ph as usize, params, &mut out);
        return Ok(out);
    }
    let n = (w * h) as usize;
    Ok(match format {
        PixelFormat::A8R8G8B8 => {
            let mut out = Vec::with_capacity(n * 4);
            for p in rgba[..n * 4].chunks(4) {
                out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
            out
        }
        PixelFormat::G8 => rgba[..n * 4].chunks(4).map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8).collect(),
        PixelFormat::V8U8 => {
            let mut out = Vec::with_capacity(n * 2);
            for p in rgba[..n * 4].chunks(4) {
                out.push((p[0] as i16 - 128) as i8 as u8);
                out.push((p[1] as i16 - 128) as i8 as u8);
            }
            out
        }
        other => return Err(Error::Format(format!("cannot encode {} from 8-bit RGBA", other.name()))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, 128, 255]);
            }
        }
        v
    }

    #[test]
    fn dds_write_read_round_trip() {
        let px = gradient(16, 8);
        let chain = rgba_mip_chain(16, 8, &px);
        assert_eq!(chain.len(), 5);
        let encoded: Vec<Vec<u8>> = chain.iter().map(|(w, h, p)| encode_rgba(PixelFormat::Dxt5, *w, *h, p).unwrap()).collect();
        let mips: Vec<DdsMip> = chain.iter().zip(&encoded).map(|((w, h, _), d)| DdsMip { width: (*w).max(4), height: (*h).max(4), data: d }).collect();
        let file = write_dds(PixelFormat::Dxt5, &mips).unwrap();
        let back = read_dds(&file).unwrap();
        assert_eq!(back.format, PixelFormat::Dxt5);
        assert_eq!((back.width, back.height, back.mips.len()), (16, 8, 5));
        assert_eq!(back.mips[0], encoded[0]);
    }

    #[test]
    fn block_decode_is_close_to_source() {
        // One flat colour per 4x4 block: only 5/6-bit quantisation error remains.
        let mut px = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let (bx, by) = (x / 4, y / 4);
                px.extend_from_slice(&[(bx * 90 + 20) as u8, (by * 120 + 40) as u8, 200, 255]);
            }
        }
        for f in [PixelFormat::Dxt1, PixelFormat::Dxt5, PixelFormat::Bc5] {
            let enc = encode_rgba(f, 8, 8, &px).unwrap();
            assert_eq!(enc.len(), f.image_bytes(8, 8));
            let dec = decode_rgba(f, 8, 8, &enc).unwrap();
            let channels = if f == PixelFormat::Bc5 { 2 } else { 3 };
            for (a, b) in px.chunks(4).zip(dec.chunks(4)) {
                for c in 0..channels {
                    assert!((a[c] as i32 - b[c] as i32).abs() <= 9, "{f:?}: {a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn uncompressed_formats_round_trip_exactly() {
        let px = gradient(5, 3);
        let enc = encode_rgba(PixelFormat::A8R8G8B8, 5, 3, &px).unwrap();
        assert_eq!(decode_rgba(PixelFormat::A8R8G8B8, 5, 3, &enc).unwrap(), px);
        let g = encode_rgba(PixelFormat::G8, 5, 3, &px).unwrap();
        assert_eq!(g.len(), 15);
        let png = write_png(5, 3, &px).unwrap();
        assert_eq!(read_png(&png).unwrap(), (5, 3, px));
    }
}
