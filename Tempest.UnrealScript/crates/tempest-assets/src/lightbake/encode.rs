//! The light map encoding of UE3 10897 (the "max component" encoding,
//! `VER_MAXCOMPONENT_LIGHTMAP_ENCODING`), unchanged in Hi-Rez's build:
//!
//! * Lightmass gathers four colours per texel (`FGatheredLightSample`): the
//!   incident light along the three tangent-space basis directions of
//!   `LightMapBasis` and along the surface normal (the "simple" light map).
//! * `ConvertToLightSample` stores three: coefficient 0 the average colour
//!   of the three directions normalised by its largest component
//!   ("NormalizedAverageColor", desaturated towards its luminance near
//!   black), coefficient 1 the largest component of each direction
//!   ("DirectionalMaxComponent", one per channel R/G/B), coefficient 2 the
//!   simple light.
//! * `QuantizeLightSamples` scales each coefficient channel by its maximum
//!   over the mapped texels (starting at 1.0, capped at 16), stores
//!   `trunc(pow(x / max, 1 / 2.2) · 255)` and keeps the maxima as the light
//!   map's `ScaleVectors`; the texel's coverage (255 mapped, 0 not) goes in
//!   alpha for the mip filter.
//! * The base pass shader (`BasePassPixelShader.usf`) samples both textures
//!   (sRGB, so a texel decodes to roughly `(byte / 255)^2.2`), multiplies by
//!   `LightMapScale[i]` and lights with
//!   `NormalizedAverageColor · dot(DirectionalMaxComponent, saturate(N·B)²)`
//!   (B the basis, N the per-pixel tangent normal: `1/3` each for the vertex
//!   normal).
//!
//! Ports: `FGatheredLightSample::PointLight` / `SkyLight` / `ConvertToLightSample`
//! (Lightmass `LightingSystem.cpp`), `QuantizeLightSamples` (`LightmapData.cpp`),
//! `FLightMapPendingTexture::StartEncoding` mips and texel expansion (`UnLightMap.cpp`).

/// `LightMapBasis` (Lightmass `LightingSystem.h`, the shader's matrix
/// transposed): three orthonormal directions around +Z, then the normal.
pub const BASIS: [[f32; 3]; 4] = [
    [0.0, 0.816_496_6, 0.577_350_26],
    [-0.707_106_77, -0.408_248_3, 0.577_350_26],
    [0.707_106_77, -0.408_248_3, 0.577_350_26],
    [0.0, 0.0, 1.0],
];

/// `MAX_LIGHT_INTENSITY` (`LightmapData.cpp`).
pub const MAX_LIGHT_INTENSITY: f32 = 16.0;
const DELTA: f32 = 0.00001;

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `FGatheredLightSample`: incident light along the three basis directions
/// and along the normal (index 3), coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gathered {
    pub c: [[f32; 3]; 4],
    pub mapped: bool,
}

impl Gathered {
    /// `FGatheredLightSample::PointLight`: light of `color` coming from the
    /// tangent-space direction `dir` (unit); `normal` is the tangent normal
    /// used for the simple coefficient (+Z without normal maps).
    pub fn point_light(color: [f32; 3], dir: [f32; 3], normal: [f32; 3]) -> Gathered {
        let mut g = Gathered::default();
        for (k, c) in g.c.iter_mut().enumerate() {
            let s = if k == 3 { dot(dir, normal).max(0.0) } else { dot(dir, BASIS[k]).max(0.0) };
            *c = [color[0] * s, color[1] * s, color[2] * s];
        }
        g
    }

    /// `FGatheredLightSample::SkyLight`: an upper and a lower hemisphere
    /// colour, `world_z` the world's +Z in tangent space.
    pub fn sky_light(upper: [f32; 3], lower: [f32; 3], world_z: [f32; 3], normal: [f32; 3]) -> Gathered {
        let mut g = Gathered::default();
        for (k, c) in g.c.iter_mut().enumerate() {
            let d = if k == 3 { dot(world_z, normal) } else { dot(world_z, BASIS[k]) };
            let up = (0.5 + 0.5 * d) * (0.5 + 0.5 * d);
            let lo = (0.5 - 0.5 * d) * (0.5 - 0.5 * d);
            *c = [upper[0] * up + lower[0] * lo, upper[1] * up + lower[1] * lo, upper[2] * up + lower[2] * lo];
        }
        g
    }

    pub fn add_weighted(&mut self, o: &Gathered, w: f32) {
        for k in 0..4 {
            for ch in 0..3 {
                self.c[k][ch] += o.c[k][ch] * w;
            }
        }
    }

    /// Scale the three directional coefficients only (the simple one keeps
    /// its cosine): `CalculatePointLighting`'s one-sided exaggeration.
    pub fn scale_directional(&mut self, s: f32) {
        for k in 0..3 {
            for ch in 0..3 {
                self.c[k][ch] *= s;
            }
        }
    }

    /// Irradiance along the normal (the simple coefficient).
    pub fn simple(&self) -> [f32; 3] {
        self.c[3]
    }
}

/// `FLightSample`: the three stored coefficients.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub c: [[f32; 3]; 3],
    pub mapped: bool,
}

/// `FLinearColor::LinearRGBToXYZ().G`.
fn luminance(c: [f32; 3]) -> f32 {
    0.212_672_9 * c[0] + 0.715_152_2 * c[1] + 0.072_175 * c[2]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `FGatheredLightSample::ConvertToLightSample`.
pub fn convert(g: &Gathered) -> Sample {
    let mut s = Sample { mapped: g.mapped, ..Default::default() };
    let mut avg = [0f32; 3];
    for k in 0..3 {
        for ch in 0..3 {
            avg[ch] += g.c[k][ch] / 3.0;
        }
    }
    let avg_lum = luminance(avg);
    let max_avg = avg[0].max(avg[1]).max(avg[2]);
    let mut dmc = [0f32; 3];
    for (k, d) in dmc.iter_mut().enumerate() {
        *d = g.c[k][0].max(g.c[k][1]).max(g.c[k][2]);
    }
    s.c[1] = dmc;
    if max_avg > 0.0 {
        let avg_max = (dmc[0] + dmc[1] + dmc[2]) / 3.0;
        let t = (avg_max * 60.0).clamp(0.0, 1.0);
        for ch in 0..3 {
            // Desaturate towards the luminance as the light approaches black.
            s.c[0][ch] = lerp(avg_lum / max_avg, avg[ch] / max_avg, t);
        }
    }
    // (With no light at all the engine's lerp is NaN, which quantizes to 0.)
    s.c[2] = g.c[3];
    s
}

/// Quantized texel (`FQuantizedLightSampleData` / `FLightMapCoefficients`):
/// three RGB bytes and the coverage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quantized {
    pub c: [[u8; 3]; 3],
    pub coverage: u8,
}

/// `QuantizeLightSamples` (2D: only mapped texels set the scale). Returns
/// the per-coefficient channel maxima (`ScaleVectors`) and the texels.
pub fn quantize(samples: &[Sample]) -> ([[f32; 3]; 3], Vec<Quantized>) {
    let mut max = [[1f32; 3]; 3];
    for s in samples.iter().filter(|s| s.mapped) {
        for k in 0..3 {
            for ch in 0..3 {
                max[k][ch] = s.c[k][ch].clamp(max[k][ch], MAX_LIGHT_INTENSITY);
            }
        }
    }
    let mut inv = [[0f32; 3]; 3];
    for k in 0..3 {
        for ch in 0..3 {
            inv[k][ch] = 1.0 / max[k][ch].max(DELTA);
        }
    }
    let q = samples
        .iter()
        .map(|s| {
            let mut out = Quantized { coverage: if s.mapped { 255 } else { 0 }, ..Default::default() };
            for k in 0..3 {
                for ch in 0..3 {
                    out.c[k][ch] = quantize_value(s.c[k][ch] * inv[k][ch]);
                }
            }
            out
        })
        .collect();
    (max, q)
}

/// `(BYTE)Clamp(appTrunc(appPow(x, 1 / 2.2) * 255), 0, 255)`.
pub fn quantize_value(x: f32) -> u8 {
    let v = x.powf(1.0 / 2.2) * 255.0;
    if v.is_nan() {
        0
    } else {
        (v as i32).clamp(0, 255) as u8
    }
}

/// The inverse of [`quantize_value`] (what the sRGB sampler approximates).
pub fn dequantize(b: u8) -> f32 {
    (b as f32 / 255.0).powf(2.2)
}

/// One decoded texel: the two stored textures in linear units (scaled).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decoded {
    pub normalized_average: [f32; 3],
    pub directional_max: [f32; 3],
}

impl Decoded {
    pub fn from_bytes(nac: [u8; 3], dmc: [u8; 3], scale: &[[f32; 3]; 3]) -> Decoded {
        let mut d = Decoded::default();
        for ch in 0..3 {
            d.normalized_average[ch] = dequantize(nac[ch]) * scale[0][ch];
            d.directional_max[ch] = dequantize(dmc[ch]) * scale[1][ch];
        }
        d
    }

    /// What the base pass lights a surface facing its vertex normal with:
    /// `NormalizedAverageColor · dot(DirectionalMaxComponent, 1/3)`.
    pub fn irradiance(&self) -> [f32; 3] {
        let k = (self.directional_max[0] + self.directional_max[1] + self.directional_max[2]) / 3.0;
        [self.normalized_average[0] * k, self.normalized_average[1] * k, self.normalized_average[2] * k]
    }

    /// The same for a baked sample before quantization.
    pub fn of_sample(s: &Sample) -> Decoded {
        Decoded { normalized_average: s.c[0], directional_max: s.c[1] }
    }
}

// ----------------------------------------------------------------------------
// Texture mips (FLightMapPendingTexture::StartEncoding)
// ----------------------------------------------------------------------------

/// RGBA8 texel with coverage in alpha (`FColor` of the pending texture).
pub type Texel = [u8; 4];

fn to_linear(c: Texel) -> [f32; 3] {
    // FLinearColor(FColor): the 2.2 power table.
    [dequantize(c[0]), dequantize(c[1]), dequantize(c[2])]
}

/// `FColor(FLinearColor)`: `appTrunc(pow(x, 1 / 2.2) · 255)` (the engine's
/// conversion back is the same quantization).
fn from_linear(c: [f32; 3]) -> [u8; 3] {
    [quantize_value(c[0]), quantize_value(c[1]), quantize_value(c[2])]
}

/// The mip chain of a coefficient texture: each mip averages the covered
/// texels of the one above (weighted by coverage), then every uncovered
/// texel takes the coverage-weighted average of its covered neighbours
/// (edges weighted 255, corners 1) so filtering never reaches black.
/// `block` is the format's block size (4 for DXT1): mips stop shrinking there.
pub fn mip_chain(w: usize, h: usize, top: Vec<Texel>, block: usize) -> Vec<(usize, usize, Vec<Texel>)> {
    let levels = (w.max(h) as f64).log2().ceil() as usize + 1;
    let mut chain: Vec<(usize, usize, Vec<Texel>)> = vec![(w, h, top)];
    for mip in 1..levels {
        let (sw, sh) = ((w >> (mip - 1)).max(block), (h >> (mip - 1)).max(block));
        let (dw, dh) = ((w >> mip).max(block), (h >> mip).max(block));
        let (fx, fy) = (sw / dw, sh / dh);
        let last = &chain[mip - 1].2;
        let mut next = vec![[0u8; 4]; dw * dh];
        for y in 0..dh {
            for x in 0..dw {
                let mut acc = [0f32; 3];
                let mut cov = 0u32;
                for sy in y * fy..(y + 1) * fy {
                    for sx in x * fx..(x + 1) * fx {
                        let c = last[sy * sw + sx];
                        if c[3] != 0 {
                            let l = to_linear(c);
                            for ch in 0..3 {
                                acc[ch] += l[ch] * c[3] as f32;
                            }
                            cov += c[3] as u32;
                        }
                    }
                }
                if cov > 0 {
                    let rgb = from_linear([acc[0] / cov as f32, acc[1] / cov as f32, acc[2] / cov as f32]);
                    next[y * dw + x] = [rgb[0], rgb[1], rgb[2], (cov / (fx * fy) as u32) as u8];
                }
            }
        }
        chain.push((dw, dh, next));
    }
    // Expand mapped texels into their unmapped neighbours.
    for (mw, mh, data) in chain.iter_mut() {
        let (mw, mh) = (*mw, *mh);
        let src = data.clone();
        const WEIGHTS: [[u32; 3]; 3] = [[1, 255, 1], [255, 0, 255], [1, 255, 1]];
        for y in 0..mh {
            for x in 0..mw {
                if src[y * mw + x][3] != 0 {
                    continue;
                }
                let mut acc = [0f32; 3];
                let mut cov = 0u64;
                // The engine's loops stop one short of +1 (`SourceX < DestX + 1`): only the left/top neighbours count.
                for sy in y.saturating_sub(1)..(y + 1).min(mh) {
                    for sx in x.saturating_sub(1)..(x + 1).min(mw) {
                        let c = src[sy * mw + sx];
                        if c[3] != 0 {
                            let wgt = c[3] as u64 * WEIGHTS[sx + 1 - x][sy + 1 - y] as u64;
                            let l = to_linear(c);
                            for ch in 0..3 {
                                acc[ch] += l[ch] * wgt as f32;
                            }
                            cov += wgt;
                        }
                    }
                }
                if cov > 0 {
                    let rgb = from_linear([acc[0] / cov as f32, acc[1] / cov as f32, acc[2] / cov as f32]);
                    data[y * mw + x] = [rgb[0], rgb[1], rgb[2], 0];
                }
            }
        }
    }
    chain
}

/// DXT1 blocks of an RGB image (alpha ignored: opaque blocks, like the
/// engine's NVTT DXT1 compression of light maps).
pub fn encode_dxt1(w: usize, h: usize, texels: &[Texel]) -> Vec<u8> {
    let (pw, ph) = (w.max(4), h.max(4));
    let mut rgba = vec![255u8; pw * ph * 4];
    for y in 0..ph {
        for x in 0..pw {
            let c = texels[y.min(h - 1) * w + x.min(w - 1)];
            rgba[(y * pw + x) * 4..][..3].copy_from_slice(&c[..3]);
        }
    }
    let f = texpresso::Format::Bc1;
    let mut out = vec![0u8; f.compressed_size(pw, ph)];
    let params = texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, weights: [1.0, 1.0, 1.0], weigh_colour_by_alpha: false };
    f.compress(&rgba, pw, ph, params, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_round_trip() {
        for b in 0..=255u8 {
            let q = quantize_value(dequantize(b));
            assert!(q == b || q + 1 == b, "{b} -> {q}");
        }
    }

    #[test]
    fn vertex_normal_reads_simple_light() {
        // A light straight above: every basis direction sees cos = 1/sqrt(3),
        // the shader's 1/3-weighted dot gives the light back... times 1/sqrt(3).
        let g = Gathered::point_light([1.0, 0.5, 0.25], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]);
        let s = convert(&g);
        let d = Decoded::of_sample(&s).irradiance();
        let k = 1.0 / 3f32.sqrt();
        for ch in 0..3 {
            assert!((d[ch] - [1.0, 0.5, 0.25][ch] * k).abs() < 1e-4, "{d:?}");
        }
        assert!((s.c[2][0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dilation_fills_neighbours() {
        let mut top = vec![[0u8; 4]; 16];
        top[5] = [200, 100, 50, 255];
        let chain = mip_chain(4, 4, top, 4);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].2[6][..3], [200, 100, 50]);
        assert_eq!(chain[0].2[6][3], 0);
    }
}
