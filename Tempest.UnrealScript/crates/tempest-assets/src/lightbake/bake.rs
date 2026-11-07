//! Direct static lighting of texture-mapped static mesh components, ported
//! from Lightmass (`TextureMapping.cpp`, `LightingSystem.cpp`, `Scene.cpp`
//! of UnrealLightmass 10897):
//!
//! * texels: the mapping's inner `SizeX - 2` square (`bPadMappings`), a
//!   texel mapped when its centre (`uv · size - 0.5` = integer) lies in a
//!   triangle of the light map channel; texels a triangle only grazes are
//!   mapped to its nearest point (`bUseConservativeTexelRasterization`);
//! * lights: `FPointLight` / `FSpotLight::GetDirectIntensity` (radial
//!   `pow(max(1 - (d/R)², 0), FalloffExponent)`, spot cone squared),
//!   directional and sky lights, intensity `FLinearColor(LightColor) ·
//!   Brightness`; shadows by rays against every shadow-casting static mesh
//!   of the map, area lights sampled over `LightSourceRadius` /
//!   `LightSourceAngle`; `CalculatePointLighting`'s one-sided
//!   `pow(max(L.z, 0), 0.85) · 2` on the directional coefficients
//!   (`DirectionalCoefficientFalloffPower/Scale` of `BaseLightmass.ini`);
//!   shadowed sky light by hemisphere rays (`CalculateSkyLighting`);
//! * padding: `PadTextureMapping` copies the edge texels outwards.
//!
//! Indirect light is an approximation, not Lightmass' photon mapping and
//! final gather: optional uniform hemisphere rays per texel, each hit
//! returning `albedo · DiffuseBoost · E_direct(hit) / π` (one bounce, a
//! constant albedo since cooked materials keep no diffuse colour the baker
//! can evaluate), escaped rays the world's `EnvironmentColor`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::bvh::{add, cross, dot, length, normalize, scale, sub, Bvh, V3};
use super::encode::{convert, Gathered, Sample};
use super::scene::{overlaps, Instance, Light, LightKind, MeshData, WorldSettings};
use crate::map::{inverse, Mat};

/// `BaseLightmass.ini` `[DevOptions.StaticLighting]`.
const VISIBILITY_RAY_OFFSET: f32 = 0.1;
const DIRECTIONAL_COEFFICIENT_FALLOFF_POWER: f32 = 0.85;
const DIRECTIONAL_COEFFICIENT_SCALE: f32 = 2.0;
const HALF_WORLD_MAX: f32 = 262_144.0;

#[derive(Clone, Debug)]
pub struct Options {
    /// Shadow rays per light and texel (area lights; 1 = hard shadows).
    pub shadow_samples: u32,
    /// Hemisphere rays for shadowed sky light (Lightmass: 128).
    pub sky_samples: u32,
    /// Hemisphere rays for the one-bounce indirect approximation (0: none).
    pub indirect_rays: u32,
    /// Diffuse reflectance assumed for every surface the bounce hits, before
    /// the world's `DiffuseBoost`. 0.08 is fitted to Lightmass: on two
    /// Shootingrange levels (Courtyard_ABC, Room_D, 48 rays) it puts the
    /// mean of shaded meshes at 0.96-1.03 of the shipped light maps (0.15:
    /// 1.15); sunlit meshes land at 0.87-1.26, per mesh within about ×1.6.
    pub albedo: f32,
    /// Scale of the world's `EnvironmentColor` that escaped rays pick up.
    pub environment_scale: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options { shadow_samples: 8, sky_samples: 64, indirect_rays: 0, albedo: 0.08, environment_scale: 1.0 }
    }
}

/// Every shadow-casting triangle of the map in world space.
pub struct Scene {
    pub lights: Vec<Light>,
    pub bvh: Bvh,
    /// World-space geometric normal of each BVH triangle.
    pub normals: Vec<V3>,
    pub world: WorldSettings,
}

fn transform_point(m: &Mat, p: V3) -> V3 {
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    [(x * m[0][0] + y * m[1][0] + z * m[2][0] + m[3][0]) as f32, (x * m[0][1] + y * m[1][1] + z * m[2][1] + m[3][1]) as f32, (x * m[0][2] + y * m[1][2] + z * m[2][2] + m[3][2]) as f32]
}

fn transform_vector(m: &Mat, p: V3) -> V3 {
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    [(x * m[0][0] + y * m[1][0] + z * m[2][0]) as f32, (x * m[0][1] + y * m[1][1] + z * m[2][1]) as f32, (x * m[0][2] + y * m[1][2] + z * m[2][2]) as f32]
}

/// `FMatrix::InverseTranspose` applied to a normal (row vectors).
fn transform_normal(inv: &Mat, n: V3) -> V3 {
    let (x, y, z) = (n[0] as f64, n[1] as f64, n[2] as f64);
    [(x * inv[0][0] + y * inv[0][1] + z * inv[0][2]) as f32, (x * inv[1][0] + y * inv[1][1] + z * inv[1][2]) as f32, (x * inv[2][0] + y * inv[2][1] + z * inv[2][2]) as f32]
}

impl Scene {
    pub fn build(lights: Vec<Light>, casters: &[(&Instance, Arc<MeshData>)], world: WorldSettings) -> Scene {
        let mut tris = Vec::new();
        let mut normals = Vec::new();
        for (inst, mesh) in casters {
            if !inst.casts_shadow {
                continue;
            }
            let p: Vec<V3> = mesh.positions.iter().map(|v| transform_point(&inst.world, *v)).collect();
            for (t, ix) in mesh.indices.chunks(3).enumerate() {
                if ix.len() < 3 || !mesh.shadow.get(t).copied().unwrap_or(true) {
                    continue;
                }
                let tri = [p[ix[0] as usize], p[ix[1] as usize], p[ix[2] as usize]];
                let n = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
                if length(n) < 1e-10 {
                    continue;
                }
                tris.push(tri);
                // Unreal winds clockwise seen from the front (left-handed).
                normals.push(normalize(scale(n, -1.0)));
            }
        }
        Scene { lights, bvh: Bvh::build(tris), normals, world }
    }
}

/// One texel of a mapping: where it is and its tangent basis.
#[derive(Clone, Copy, Debug, Default)]
pub struct Texel {
    pub mapped: bool,
    pub position: V3,
    pub tx: V3,
    pub ty: V3,
    pub tz: V3,
    /// Texel size in world units (for ray offsets).
    pub radius: f32,
}

/// A component to bake.
pub struct Target<'a> {
    pub instance: &'a Instance,
    pub mesh: Arc<MeshData>,
    /// Light map size with padding (`GetLightMapResolution`).
    pub size: usize,
}

/// The light map resolution of a component, `None` when it cannot have a
/// texture light map (no light map UVs, no resolution).
pub fn resolution(inst: &Instance, mesh: &MeshData) -> Option<usize> {
    let r = inst.override_resolution.unwrap_or(mesh.resolution);
    (r > 0 && mesh.has_lightmap_uvs).then_some(r as usize)
}

fn bary(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<(f32, f32, f32)> {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let w0 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let w1 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    Some((w0, w1, 1.0 - w0 - w1))
}

fn closest_on_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> ([f32; 2], f32) {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let l = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l).clamp(0.0, 1.0) } else { 0.0 };
    ([a[0] + ab[0] * t, a[1] + ab[1] * t], t)
}

/// Rasterize the light map channel of `mesh` placed by `world` into an
/// `n`×`n` grid (the unpadded mapping).
pub fn rasterize(mesh: &MeshData, world: &Mat, n: usize) -> Vec<Texel> {
    let inv = inverse(world);
    let pos: Vec<V3> = mesh.positions.iter().map(|v| transform_point(world, *v)).collect();
    let tx: Vec<V3> = mesh.tangent_x.iter().map(|v| transform_vector(world, *v)).collect();
    let ty: Vec<V3> = mesh.tangent_y.iter().map(|v| transform_vector(world, *v)).collect();
    let tz: Vec<V3> = mesh.tangent_z.iter().map(|v| transform_normal(&inv, *v)).collect();
    let mut out = vec![Texel::default(); n * n];
    // Distance (texels) of the nearest triangle for texels no centre covers.
    let mut near = vec![f32::INFINITY; n * n];
    let nf = n as f32;
    for ix in mesh.indices.chunks(3) {
        if ix.len() < 3 {
            continue;
        }
        let v = [ix[0] as usize, ix[1] as usize, ix[2] as usize];
        let uv = v.map(|i| [mesh.uvs[i][0] * nf - 0.5, mesh.uvs[i][1] * nf - 0.5]);
        let area3 = length(cross(sub(pos[v[1]], pos[v[0]]), sub(pos[v[2]], pos[v[0]])));
        let area2 = ((uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1]) - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1])).abs();
        if area2 < 1e-9 {
            continue;
        }
        let texel_radius = (area3 / area2).sqrt() * 0.5;
        let minx = uv.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64 - 1;
        let maxx = uv.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(nf - 1.0) as i64 + 1;
        let miny = uv.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i64 - 1;
        let maxy = uv.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(nf - 1.0) as i64 + 1;
        for y in miny.max(0)..=maxy.min(n as i64 - 1) {
            for x in minx.max(0)..=maxx.min(n as i64 - 1) {
                let p = [x as f32, y as f32];
                let Some((mut a, mut b, mut c)) = bary(p, uv[0], uv[1], uv[2]) else { continue };
                let k = y as usize * n + x as usize;
                let inside = a >= -1e-5 && b >= -1e-5 && c >= -1e-5;
                if !inside {
                    if out[k].mapped && near[k] == 0.0 {
                        continue;
                    }
                    // Nearest point of the triangle, within half a texel diagonal.
                    let mut best = ([0f32; 2], f32::INFINITY);
                    for (e0, e1) in [(0, 1), (1, 2), (2, 0)] {
                        let (q, _) = closest_on_segment(p, uv[e0], uv[e1]);
                        let d = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
                        if d < best.1 {
                            best = (q, d);
                        }
                    }
                    if best.1 > 0.71 || best.1 >= near[k] {
                        continue;
                    }
                    near[k] = best.1;
                    let Some(w) = bary(best.0, uv[0], uv[1], uv[2]) else { continue };
                    (a, b, c) = (w.0.clamp(0.0, 1.0), w.1.clamp(0.0, 1.0), w.2.clamp(0.0, 1.0));
                } else {
                    near[k] = 0.0;
                }
                let mix = |f: &[V3]| add(add(scale(f[v[0]], a), scale(f[v[1]], b)), scale(f[v[2]], c));
                let z = normalize(mix(&tz));
                if z == [0.0; 3] {
                    continue;
                }
                out[k] = Texel { mapped: true, position: mix(&pos), tx: normalize(mix(&tx)), ty: normalize(mix(&ty)), tz: z, radius: texel_radius };
            }
        }
    }
    out
}

/// Small deterministic generator (xorshift*), seeded per texel.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
    fn unit(&mut self) -> V3 {
        // `GetUnitVector`: rejection sampling in the cube.
        loop {
            let v = [self.next() * 2.0 - 1.0, self.next() * 2.0 - 1.0, self.next() * 2.0 - 1.0];
            let l = dot(v, v);
            if l > 1e-4 && l <= 1.0 {
                return scale(v, 1.0 / l.sqrt());
            }
        }
    }
}

fn to_tangent(t: &Texel, w: V3) -> V3 {
    [dot(w, t.tx), dot(w, t.ty), dot(w, t.tz)]
}

/// Direct intensity of a point-like or directional light at `p`, with the
/// direction towards it: `F*Light::GetDirectIntensity`.
fn direct(l: &Light, p: V3) -> Option<([f32; 3], V3, f32)> {
    match &l.kind {
        LightKind::Point | LightKind::Spot { .. } => {
            let d = sub(l.position, p);
            let dist = length(d);
            if dist >= l.radius || dist <= 0.0 {
                return None;
            }
            let radial = (1.0 - (dist / l.radius).powi(2)).max(0.0).powf(l.falloff);
            let mut att = radial;
            if let LightKind::Spot { inner, outer } = l.kind {
                let ci = inner.clamp(0.0, 89.0).to_radians();
                let co = outer.to_radians().clamp(ci + 0.001, 89f32.to_radians() + 0.001);
                let (outer_c, inner_c) = (co.cos(), ci.cos());
                let lv = scale(d, -1.0 / dist);
                let s = ((dot(lv, l.direction) - outer_c) / (inner_c - outer_c)).clamp(0.0, 1.0);
                att *= s * s;
            }
            (att > 0.0).then(|| (scale(l.intensity, att), scale(d, 1.0 / dist), dist))
        }
        LightKind::Directional => Some((l.intensity, scale(l.direction, -1.0), HALF_WORLD_MAX)),
        LightKind::Sky { .. } => None,
    }
}

/// An orthonormal pair perpendicular to `w`.
fn frame(w: V3) -> (V3, V3) {
    let a = if w[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let u = normalize(cross(w, a));
    (u, cross(w, u))
}

/// Fraction of the light visible from `start` (already offset).
fn transmission(scene: &Scene, l: &Light, start: V3, w: V3, samples: u32, rng: &mut Rng) -> f32 {
    if !l.casts_static_shadows {
        return 1.0;
    }
    let n = samples.max(1);
    let mut visible = 0;
    for s in 0..n {
        let (dir, len) = match l.kind {
            LightKind::Directional => {
                let half = l.source_angle * 0.5;
                if n == 1 || half <= 0.0 {
                    (w, HALF_WORLD_MAX)
                } else {
                    let (u, v) = frame(w);
                    let r = half * rng.next().sqrt();
                    let a = std::f32::consts::TAU * (s as f32 + rng.next()) / n as f32;
                    (normalize(add(w, add(scale(u, r * a.cos()), scale(v, r * a.sin())))), HALF_WORLD_MAX)
                }
            }
            _ => {
                let target = if n == 1 || l.source_radius <= 0.0 { l.position } else { add(l.position, scale(rng.unit(), l.source_radius * rng.next().cbrt())) };
                let d = sub(target, start);
                let len = length(d);
                (scale(d, 1.0 / len.max(1e-6)), len)
            }
        };
        if !scene.bvh.occluded(start, dir, (len - VISIBILITY_RAY_OFFSET).max(0.0)) {
            visible += 1;
        }
    }
    visible as f32 / n as f32
}

/// Direct irradiance along `normal` at a point (one shadow ray per light):
/// what a bounce reflects.
fn irradiance_at(scene: &Scene, p: V3, normal: V3, rng: &mut Rng) -> [f32; 3] {
    let mut e = [0f32; 3];
    for l in &scene.lights {
        if let LightKind::Sky { lower } = &l.kind {
            let d = normal[2];
            let up = (0.5 + 0.5 * d).powi(2);
            let lo = (0.5 - 0.5 * d).powi(2);
            for ch in 0..3 {
                e[ch] += (l.intensity[ch] * up + lower[ch] * lo) * l.indirect_scale;
            }
            continue;
        }
        let Some((i, w, _)) = direct(l, p) else { continue };
        let c = dot(w, normal);
        if c <= 0.0 {
            continue;
        }
        let start = add(p, add(scale(normal, 1.0), scale(w, VISIBILITY_RAY_OFFSET)));
        let t = transmission(scene, l, start, w, 1, rng);
        for ch in 0..3 {
            e[ch] += i[ch] * c * t * l.indirect_scale;
        }
    }
    e
}

/// Light one texel: every relevant light's `FGatheredLightSample`.
/// `lit[k]` is set when light `k` added something.
fn light_texel(scene: &Scene, t: &Texel, inst: &Instance, opts: &Options, rng: &mut Rng, lit: &mut [bool]) -> Gathered {
    let mut g = Gathered { mapped: true, ..Default::default() };
    let n = t.tz;
    let normal_offset = t.radius.clamp(0.5, 3.0);
    let up = [t.tx[2], t.ty[2], t.tz[2]];
    for (k, l) in scene.lights.iter().enumerate() {
        if !overlaps(&l.channels, &inst.channels) {
            continue;
        }
        if let LightKind::Sky { lower } = &l.kind {
            if !l.casts_static_shadows {
                g.add_weighted(&Gathered::sky_light(l.intensity, *lower, up, [0.0, 0.0, 1.0]), 1.0);
            } else {
                // CalculateSkyLighting: rays over the sphere, the upper or
                // lower hemisphere colour for each one that escapes.
                let count = opts.sky_samples.max(1);
                for _ in 0..count {
                    let w = rng.unit();
                    let td = to_tangent(t, w);
                    if td[2] <= 0.0 {
                        continue;
                    }
                    let start = add(t.position, add(scale(n, normal_offset), scale(w, VISIBILITY_RAY_OFFSET)));
                    if !scene.bvh.occluded(start, w, HALF_WORLD_MAX) {
                        let c = if w[2] > 0.0 { l.intensity } else { *lower };
                        g.add_weighted(&Gathered::point_light(c, td, [0.0, 0.0, 1.0]), 2.0 / count as f32);
                    }
                }
            }
            lit[k] = true;
            continue;
        }
        let Some((intensity, w, _)) = direct(l, t.position) else { continue };
        let tl = normalize(to_tangent(t, w));
        if tl[2] <= 0.0 && !inst.two_sided {
            continue;
        }
        let start = add(t.position, add(scale(n, if tl[2] > 0.0 { normal_offset } else { -normal_offset }), scale(w, VISIBILITY_RAY_OFFSET)));
        let tr = transmission(scene, l, start, w, opts.shadow_samples, rng);
        if tr <= 0.0 {
            continue;
        }
        let c = scale(intensity, tr);
        let mut s = Gathered::point_light(c, tl, [0.0, 0.0, 1.0]);
        if inst.two_sided {
            let back = Gathered::point_light(c, scale(tl, -1.0), [0.0, 0.0, 1.0]);
            s.add_weighted(&back, 1.0);
            for k2 in 0..4 {
                for ch in 0..3 {
                    s.c[k2][ch] *= 0.5;
                }
            }
        } else {
            s.scale_directional(tl[2].max(0.0).powf(DIRECTIONAL_COEFFICIENT_FALLOFF_POWER) * DIRECTIONAL_COEFFICIENT_SCALE);
        }
        g.add_weighted(&s, 1.0);
        lit[k] = true;
    }
    if opts.indirect_rays > 0 {
        let count = opts.indirect_rays;
        let rho = (opts.albedo * scene.world.diffuse_boost).min(1.0);
        let mut indirect = Gathered::default();
        let mut occluded = 0u32;
        for _ in 0..count {
            // Uniform over the hemisphere: E = 2π/N Σ L cos θ, and a
            // `PointLight` sample already carries the cosine.
            let mut w = rng.unit();
            if dot(w, n) < 0.0 {
                w = scale(w, -1.0);
            }
            let td = to_tangent(t, w);
            let start = add(t.position, add(scale(n, normal_offset), scale(w, VISIBILITY_RAY_OFFSET)));
            let color = match scene.bvh.closest(start, w, HALF_WORLD_MAX) {
                Some(h) => {
                    if h.t < scene.world.max_occlusion_distance {
                        occluded += 1;
                    }
                    let hn = scene.normals[h.tri as usize];
                    if dot(hn, w) >= 0.0 {
                        continue;
                    }
                    let hp = add(start, scale(w, h.t));
                    let e = irradiance_at(scene, hp, hn, rng);
                    // L = ρ E / π; 2π L / N = 2 ρ E / N.
                    [e[0] * rho * 2.0, e[1] * rho * 2.0, e[2] * rho * 2.0]
                }
                None => scale(scene.world.environment, 2.0 * opts.environment_scale),
            };
            indirect.add_weighted(&Gathered::point_light(color, td, [0.0, 0.0, 1.0]), 1.0 / count as f32);
        }
        // Ambient occlusion (`IncomingRadianceUniform` + the texture mapping's
        // final gather): the fraction of rays hitting within
        // `MaxOcclusionDistance` darkens direct and indirect light.
        let (mut direct_k, mut indirect_k) = (1.0, 1.0);
        let w = &scene.world;
        if w.ambient_occlusion {
            let frac = (occluded as f32 / (w.fully_occluded_fraction.max(1e-3) * count as f32)).min(1.0);
            let occ = (0.5 * (w.occlusion_exponent + 1.0) * frac.powf(w.occlusion_exponent)).clamp(0.0, 1.0);
            direct_k = 1.0 - w.direct_occlusion * occ;
            indirect_k = 1.0 - w.indirect_occlusion * occ;
        }
        for k in 0..4 {
            for ch in 0..3 {
                g.c[k][ch] = g.c[k][ch] * direct_k + indirect.c[k][ch] * indirect_k;
            }
        }
    }
    g
}

/// `PadTextureMapping`: the inner `n`×`n` samples with one texel copied
/// outwards on every side.
pub fn pad(inner: &[Gathered], n: usize) -> Vec<Gathered> {
    let size = n + 2;
    let mut out = vec![Gathered::default(); size * size];
    for y in 0..size {
        let sy = y.saturating_sub(1).min(n - 1);
        for x in 0..size {
            let sx = x.saturating_sub(1).min(n - 1);
            out[y * size + x] = inner[sy * n + sx];
        }
    }
    out
}

/// A baked component: `size`×`size` samples (padding included) and the
/// lights it holds.
pub struct Baked {
    pub size: usize,
    pub gathered: Vec<Gathered>,
    pub samples: Vec<Sample>,
    /// Indices into the scene's lights that lit at least one texel.
    pub lights: Vec<usize>,
    pub mapped: usize,
}

/// Bake one component. `rows_done` counts finished rows for progress.
pub fn bake(scene: &Scene, target: &Target, opts: &Options, rows_done: &AtomicUsize) -> Baked {
    let size = target.size;
    let padded = size > 2;
    let n = if padded { size - 2 } else { size };
    let texels = rasterize(&target.mesh, &target.instance.world, n);
    let rows: Vec<usize> = (0..n).collect();
    let seed = target.instance.export as u64 * 7919 + target.instance.level as u64 * 104_729;
    let results = tempest_unrealscript::par::map(&rows, |&y| {
        let mut lit = vec![false; scene.lights.len()];
        let mut row = Vec::with_capacity(n);
        for x in 0..n {
            let t = &texels[y * n + x];
            if !t.mapped {
                row.push(Gathered::default());
                continue;
            }
            let mut rng = Rng::new(seed ^ ((y * n + x) as u64).wrapping_mul(0x1000_0000_01B3));
            row.push(light_texel(scene, t, target.instance, opts, &mut rng, &mut lit));
        }
        rows_done.fetch_add(1, Ordering::Relaxed);
        (row, lit)
    });
    let mut inner = Vec::with_capacity(n * n);
    let mut lit = vec![false; scene.lights.len()];
    for (row, l) in results {
        inner.extend(row);
        for (a, b) in lit.iter_mut().zip(l) {
            *a |= b;
        }
    }
    let mapped = inner.iter().filter(|g| g.mapped).count();
    let gathered = if padded { pad(&inner, n) } else { inner };
    let samples = gathered.iter().map(convert).collect();
    Baked { size, gathered, samples, lights: lit.iter().enumerate().filter(|(_, b)| **b).map(|(i, _)| i).collect(), mapped }
}
