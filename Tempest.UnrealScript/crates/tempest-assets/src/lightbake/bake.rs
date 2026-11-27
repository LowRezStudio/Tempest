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
//!
//! The path-traced mode (`Options::path_traced`, [`bake_traced`]) traces
//! in batches through a [`Tracer`] (the CPU tree, or a GPU): the direct
//! light is the same as above (each texel's rays recorded, traced together,
//! then replayed), the indirect light follows each hemisphere ray for up to
//! `bounces` bounces (cosine-weighted after the first, Russian roulette from
//! the third), each hit reflecting the direct light there (a shadow ray per
//! light, the sky unshadowed) times the surface's colour: its material's
//! diffuse texture at the hit, scaled so the scene's mean is `albedo` (the
//! level fitted to Lightmass), times `DiffuseBoost`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::bvh::{add, cross, dot, length, normalize, scale, sub, Bvh, Hit, V3};
use super::encode::{convert, Gathered, Sample};
use super::trace::{Ray, Tracer};
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
    /// Path traced, the scene's mean (the materials' colours scaled to it),
    /// fitted with `environment_scale` below.
    pub albedo: f32,
    /// Scale of the world's `EnvironmentColor` that escaped rays pick up.
    /// Path traced, 0.1 (with albedo 0.08, 3 bounces): at 1 Shootingrange's
    /// cyan environment (99, 204, 236) × 4.45 tints the shade blue-green
    /// (shaded texels' chroma 0.37/0.34/0.29 against the shipped maps'
    /// 0.46/0.30/0.24, which equals our direct light's: Lightmass' escaped
    /// rays add next to nothing there; `bEnableAdvancedEnvironmentColor` is
    /// set and its rule is not ported). At 0.1: 0.45/0.33/0.22, mean
    /// |difference| 0.375 and 0.250 on Room_D and Courtyard_ABC (0.364 and
    /// 0.289 for one grey bounce).
    pub environment_scale: f32,
    /// Trace in batches with multi-bounce indirect light and material colours
    /// ([`bake_traced`]); `indirect_rays` is then the paths per texel.
    pub path_traced: bool,
    /// Bounces of a path (path-traced mode).
    pub bounces: u32,
    /// Path traced: the indirect light is averaged over this many texels
    /// around each (on the same surface: weighted by distance and normal),
    /// the direct light left sharp. 0: no filtering.
    pub denoise_radius: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options { shadow_samples: 8, sky_samples: 64, indirect_rays: 0, albedo: 0.08, environment_scale: 1.0, path_traced: false, bounces: 3, denoise_radius: 2 }
    }
}

/// A material's diffuse colour: its base colour texture, small, in linear light.
#[derive(Debug)]
pub struct Albedo {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<[f32; 3]>,
}

impl Albedo {
    /// Nearest texel at `uv`, wrapping.
    pub fn at(&self, uv: [f32; 2]) -> [f32; 3] {
        if self.texels.is_empty() {
            return [0.0; 3];
        }
        let x = ((uv[0] - uv[0].floor()) * self.width as f32) as usize % self.width;
        let y = ((uv[1] - uv[1].floor()) * self.height as f32) as usize % self.height;
        self.texels[y * self.width + x]
    }

    pub fn mean(&self) -> [f32; 3] {
        let n = self.texels.len().max(1) as f32;
        let mut m = [0f32; 3];
        for t in &self.texels {
            for k in 0..3 {
                m[k] += t[k] / n;
            }
        }
        m
    }
}

/// What a BVH triangle is made of: an index into the scene's albedos
/// (`u32::MAX`: none known) and its corners' texture coordinate 0.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub albedo: u32,
    pub uv: [[f32; 2]; 3],
}

/// Every shadow-casting triangle of the map in world space.
pub struct Scene {
    pub lights: Vec<Light>,
    pub bvh: Bvh,
    /// World-space geometric normal of each BVH triangle.
    pub normals: Vec<V3>,
    pub world: WorldSettings,
    /// Per BVH triangle (path-traced mode), and the albedos they name.
    pub surfaces: Vec<Surface>,
    pub albedos: Vec<Arc<Albedo>>,
    /// Multiplies every albedo so their area-weighted mean is `Options::albedo`.
    pub albedo_scale: f32,
    /// The area-weighted mean luminance of the albedos over the triangles
    /// that have one (`None`: no triangle has).
    pub albedo_mean: Option<f32>,
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
    /// `albedo(instance, mesh, element)`: the albedo of an element of a
    /// caster (an index into `albedos`), for the path-traced mode.
    pub fn build(lights: Vec<Light>, casters: &[(&Instance, Arc<MeshData>)], world: WorldSettings, albedos: Vec<Arc<Albedo>>, albedo: &dyn Fn(&Instance, &MeshData, usize) -> Option<u32>) -> Scene {
        let mut tris = Vec::new();
        let mut normals = Vec::new();
        let mut surfaces = Vec::new();
        let means: Vec<[f32; 3]> = albedos.iter().map(|a| a.mean()).collect();
        let (mut lum_sum, mut lum_area) = (0f64, 0f64);
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
                if !albedos.is_empty() {
                    let element = mesh.tri_element.get(t).copied().unwrap_or(0) as usize;
                    let uv = |k: usize| mesh.uv0.get(ix[k] as usize).copied().unwrap_or([0.0; 2]);
                    let a = albedo(inst, mesh, element).unwrap_or(u32::MAX);
                    if let Some(m) = means.get(a as usize) {
                        let area = length(n) as f64 * 0.5;
                        lum_sum += area * (0.2126 * m[0] + 0.7152 * m[1] + 0.0722 * m[2]) as f64;
                        lum_area += area;
                    }
                    surfaces.push(Surface { albedo: a, uv: [uv(0), uv(1), uv(2)] });
                }
            }
        }
        let albedo_mean = (lum_area > 0.0 && lum_sum > 0.0).then(|| (lum_sum / lum_area) as f32);
        Scene { lights, bvh: Bvh::build(tris), normals, world, surfaces, albedos, albedo_scale: 1.0, albedo_mean }
    }

    /// The diffuse reflectance at a hit, `DiffuseBoost` applied.
    fn reflectance(&self, h: &Hit, opts: &Options) -> [f32; 3] {
        let boost = self.world.diffuse_boost;
        let c = match self.surfaces.get(h.tri as usize).and_then(|s| self.albedos.get(s.albedo as usize).map(|a| (s, a))) {
            Some((s, a)) => {
                let w = 1.0 - h.u - h.v;
                let uv = [s.uv[0][0] * w + s.uv[1][0] * h.u + s.uv[2][0] * h.v, s.uv[0][1] * w + s.uv[1][1] * h.u + s.uv[2][1] * h.v];
                let c = a.at(uv);
                [c[0] * self.albedo_scale, c[1] * self.albedo_scale, c[2] * self.albedo_scale]
            }
            None => [opts.albedo; 3],
        };
        c.map(|x| (x * boost).clamp(0.0, 0.98))
    }
}

/// Visibility answers: straight from the tree, or recorded (every query
/// visible) and then replayed from a batch's results. The direct light asks
/// the same rays whatever the answers, so a replay gives what a direct walk
/// would.
enum Vis<'a> {
    Tree(&'a Bvh),
    Record(Vec<Ray>),
    Replay(&'a [bool], usize),
}

impl Vis<'_> {
    fn occluded(&mut self, o: V3, d: V3, tmax: f32) -> bool {
        match self {
            Vis::Tree(b) => b.occluded(o, d, tmax),
            Vis::Record(v) => {
                v.push(Ray { o, d, tmax });
                false
            }
            Vis::Replay(a, i) => {
                let r = a.get(*i).copied().unwrap_or(false);
                *i += 1;
                r
            }
        }
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
#[derive(Clone)]
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
fn transmission(vis: &mut Vis, l: &Light, start: V3, w: V3, samples: u32, rng: &mut Rng) -> f32 {
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
        if !vis.occluded(start, dir, (len - VISIBILITY_RAY_OFFSET).max(0.0)) {
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
        let t = transmission(&mut Vis::Tree(&scene.bvh), l, start, w, 1, rng);
        for ch in 0..3 {
            e[ch] += i[ch] * c * t * l.indirect_scale;
        }
    }
    e
}

/// Light one texel: every relevant light's `FGatheredLightSample`.
/// `lit[k]` is set when light `k` added something.
fn light_texel(scene: &Scene, t: &Texel, inst: &Instance, opts: &Options, rng: &mut Rng, lit: &mut [bool], vis: &mut Vis) -> Gathered {
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
                    if !vis.occluded(start, w, HALF_WORLD_MAX) {
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
        let tr = transmission(vis, l, start, w, opts.shadow_samples, rng);
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
            row.push(light_texel(scene, t, target.instance, opts, &mut rng, &mut lit, &mut Vis::Tree(&scene.bvh)));
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

/// One path of the path-traced indirect light.
struct Path {
    /// Index into the bake's mapped texels.
    texel: usize,
    /// The first direction, in the texel's tangent space.
    first: V3,
    /// Throughput, and radiance gathered so far (the reflected direct light).
    weight: [f32; 3],
    radiance: [f32; 3],
    rng: Rng,
}

/// A cosine-weighted direction around `n`.
fn cosine_dir(n: V3, rng: &mut Rng) -> V3 {
    let (u, v) = frame(n);
    let r = rng.next().sqrt();
    let a = std::f32::consts::TAU * rng.next();
    let z = (1.0 - r * r).max(0.0).sqrt();
    normalize(add(scale(n, z), add(scale(u, r * a.cos()), scale(v, r * a.sin()))))
}

/// The indirect light of each mapped texel averaged with its neighbours in
/// the map within `radius` texels that lie on the same surface: weights fall
/// with the world distance (a Gaussian of two texel sizes) and the normals'
/// agreement (`cos⁸`), so charts that touch in the map but not in the world,
/// and creases, keep apart.
fn denoise(texels: &[Texel], n: usize, mapped: &[usize], indirect: &[Gathered], radius: i64) -> Vec<Gathered> {
    let mut at = vec![usize::MAX; texels.len()];
    for (k, &i) in mapped.iter().enumerate() {
        at[i] = k;
    }
    let jobs: Vec<usize> = (0..mapped.len()).collect();
    tempest_unrealscript::par::map(&jobs, |&k| {
        let i = mapped[k];
        let (x, y) = ((i % n) as i64, (i / n) as i64);
        let t = &texels[i];
        let sigma2 = (2.0 * t.radius.max(1e-3)).powi(2);
        let mut sum = Gathered { mapped: true, ..Default::default() };
        let mut total = 0f32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (xx, yy) = (x + dx, y + dy);
                if xx < 0 || yy < 0 || xx >= n as i64 || yy >= n as i64 {
                    continue;
                }
                let j = at[(yy as usize) * n + xx as usize];
                if j == usize::MAX {
                    continue;
                }
                let o = &texels[mapped[j]];
                let c = dot(t.tz, o.tz);
                if c <= 0.0 {
                    continue;
                }
                let d = sub(t.position, o.position);
                let w = (-dot(d, d) / (2.0 * sigma2)).exp() * c.powi(8);
                sum.add_weighted(&indirect[j], w);
                total += w;
            }
        }
        if total > 0.0 {
            for c in sum.c.iter_mut() {
                for v in c.iter_mut() {
                    *v /= total;
                }
            }
            sum
        } else {
            indirect[k]
        }
    })
}

/// [`bake`] traced in batches through `tracer`, with multi-bounce indirect
/// light coloured by the materials (see the module comment).
pub fn bake_traced(scene: &Scene, target: &Target, opts: &Options, tracer: &dyn Tracer, rows_done: &AtomicUsize) -> Baked {
    let size = target.size;
    let padded = size > 2;
    let n = if padded { size - 2 } else { size };
    let texels = rasterize(&target.mesh, &target.instance.world, n);
    let seed = target.instance.export as u64 * 7919 + target.instance.level as u64 * 104_729;
    let mapped: Vec<usize> = (0..texels.len()).filter(|&i| texels[i].mapped).collect();
    let rng_of = |i: usize| Rng::new(seed ^ (i as u64).wrapping_mul(0x1000_0000_01B3));
    let inst = target.instance;
    let nl = scene.lights.len();
    // Direct light: each texel's rays recorded, traced together, replayed.
    let direct_opts = Options { indirect_rays: 0, ..opts.clone() };
    let recorded: Vec<Vec<Ray>> = tempest_unrealscript::par::map(&mapped, |&i| {
        let mut vis = Vis::Record(Vec::new());
        light_texel(scene, &texels[i], inst, &direct_opts, &mut rng_of(i), &mut vec![false; nl], &mut vis);
        match vis {
            Vis::Record(r) => r,
            _ => Vec::new(),
        }
    });
    let offsets: Vec<usize> = recorded.iter().scan(0, |at, r| {
        let o = *at;
        *at += r.len();
        Some(o)
    }).collect();
    let answers = tracer.occluded(&recorded.concat());
    let jobs: Vec<(usize, usize)> = mapped.iter().copied().zip(offsets).collect();
    let directs: Vec<(Gathered, Vec<bool>, Rng)> = tempest_unrealscript::par::map(&jobs, |&(i, at)| {
        let mut rng = rng_of(i);
        let mut lit = vec![false; nl];
        let g = light_texel(scene, &texels[i], inst, &direct_opts, &mut rng, &mut lit, &mut Vis::Replay(&answers[at..], 0));
        (g, lit, rng)
    });
    rows_done.fetch_add(n / 2, Ordering::Relaxed);
    // Indirect light: paths, all advanced one bounce at a time.
    let count = opts.indirect_rays;
    let mut indirect: Vec<Gathered> = vec![Gathered::default(); mapped.len()];
    let mut occluded = vec![0u32; mapped.len()];
    if count > 0 {
        let mut paths: Vec<Path> = Vec::with_capacity(mapped.len() * count as usize);
        let mut rays: Vec<Ray> = Vec::with_capacity(paths.capacity());
        for (k, &i) in mapped.iter().enumerate() {
            let t = &texels[i];
            let normal_offset = t.radius.clamp(0.5, 3.0);
            let mut rng = directs[k].2.clone();
            // Uniform over the hemisphere as the one-bounce mode, stratified:
            // one direction per cell of a grid over (cos θ, φ).
            let cols = (count as f32).sqrt().ceil().max(1.0) as u32;
            let rows = count.div_ceil(cols);
            let (u, v) = frame(t.tz);
            for j in 0..count {
                let z = ((j / cols) as f32 + rng.next()) / rows as f32;
                let phi = std::f32::consts::TAU * ((j % cols) as f32 + rng.next()) / cols as f32;
                let r = (1.0 - z * z).max(0.0).sqrt();
                let w = normalize(add(scale(t.tz, z), add(scale(u, r * phi.cos()), scale(v, r * phi.sin()))));
                rays.push(Ray { o: add(t.position, add(scale(t.tz, normal_offset), scale(w, VISIBILITY_RAY_OFFSET))), d: w, tmax: HALF_WORLD_MAX });
                paths.push(Path { texel: k, first: to_tangent(t, w), weight: [1.0; 3], radiance: [0.0; 3], rng: Rng::new(rng.0 ^ 0x5851_F42D_4C95_7F2D) });
                rng.next();
            }
        }
        // Escaped rays see the environment: L = EnvironmentColor · scale / π
        // (the one-bounce mode's 2πL = 2 · EnvironmentColor · scale).
        let env = scale(scene.world.environment, opts.environment_scale / std::f32::consts::PI);
        let mut alive: Vec<usize> = (0..paths.len()).collect();
        for bounce in 0..opts.bounces.max(1) {
            if alive.is_empty() {
                break;
            }
            let hits = tracer.closest(&rays);
            let mut next_alive = Vec::new();
            let mut next_rays = Vec::new();
            // (path, contribution) of each shadow ray towards a light.
            let mut nee: Vec<(usize, [f32; 3])> = Vec::new();
            let mut nee_rays = Vec::new();
            for (&pi, (ray, hit)) in alive.iter().zip(rays.iter().zip(hits)) {
                let p = &mut paths[pi];
                let Some(h) = hit else {
                    for ((r, w), e) in p.radiance.iter_mut().zip(p.weight).zip(env) {
                        *r += w * e;
                    }
                    continue;
                };
                if bounce == 0 && h.t < scene.world.max_occlusion_distance {
                    occluded[p.texel] += 1;
                }
                let hn = scene.normals[h.tri as usize];
                if dot(hn, ray.d) >= 0.0 {
                    continue;
                }
                let hp = add(ray.o, scale(ray.d, h.t));
                let rho = scene.reflectance(&h, opts);
                // The direct light at the hit, reflected: ρ/π · E.
                let k = [p.weight[0] * rho[0] / std::f32::consts::PI, p.weight[1] * rho[1] / std::f32::consts::PI, p.weight[2] * rho[2] / std::f32::consts::PI];
                for l in &scene.lights {
                    if let LightKind::Sky { lower } = &l.kind {
                        let d = hn[2];
                        let (up, lo) = ((0.5 + 0.5 * d).powi(2), (0.5 - 0.5 * d).powi(2));
                        for ch in 0..3 {
                            p.radiance[ch] += k[ch] * (l.intensity[ch] * up + lower[ch] * lo) * l.indirect_scale;
                        }
                        continue;
                    }
                    let Some((i, w, dist)) = direct(l, hp) else { continue };
                    let c = dot(w, hn);
                    if c <= 0.0 {
                        continue;
                    }
                    let e = [k[0] * i[0] * c * l.indirect_scale, k[1] * i[1] * c * l.indirect_scale, k[2] * i[2] * c * l.indirect_scale];
                    if !l.casts_static_shadows {
                        for (r, x) in p.radiance.iter_mut().zip(e) {
                            *r += x;
                        }
                        continue;
                    }
                    let start = add(hp, add(scale(hn, 1.0), scale(w, VISIBILITY_RAY_OFFSET)));
                    nee.push((pi, e));
                    nee_rays.push(Ray { o: start, d: w, tmax: (dist - 1.0 - VISIBILITY_RAY_OFFSET).max(0.0) });
                }
                // On: cosine-weighted, the throughput times ρ (the pdf cancels
                // the cosine and 1/π); Russian roulette from the third bounce.
                if bounce + 1 < opts.bounces {
                    let mut wgt = [p.weight[0] * rho[0], p.weight[1] * rho[1], p.weight[2] * rho[2]];
                    if bounce >= 2 {
                        let q = wgt[0].max(wgt[1]).max(wgt[2]).min(0.95);
                        if p.rng.next() >= q {
                            continue;
                        }
                        wgt = wgt.map(|x| x / q);
                    }
                    p.weight = wgt;
                    let d = cosine_dir(hn, &mut p.rng);
                    next_rays.push(Ray { o: add(hp, add(scale(hn, 0.5), scale(d, VISIBILITY_RAY_OFFSET))), d, tmax: HALF_WORLD_MAX });
                    next_alive.push(pi);
                }
            }
            for ((pi, e), blocked) in nee.into_iter().zip(tracer.occluded(&nee_rays)) {
                if !blocked {
                    for (r, x) in paths[pi].radiance.iter_mut().zip(e) {
                        *r += x;
                    }
                }
            }
            alive = next_alive;
            rays = next_rays;
        }
        // 2πL over N uniform hemisphere rays, as the one-bounce mode.
        for p in &paths {
            let c = scale(p.radiance, std::f32::consts::TAU);
            indirect[p.texel].add_weighted(&Gathered::point_light(c, p.first, [0.0, 0.0, 1.0]), 1.0 / count as f32);
        }
        if opts.denoise_radius > 0 {
            indirect = denoise(&texels, n, &mapped, &indirect, opts.denoise_radius as i64);
        }
    }
    let w = &scene.world;
    let mut inner = vec![Gathered::default(); n * n];
    let mut lit = vec![false; nl];
    for (k, &i) in mapped.iter().enumerate() {
        let mut g = directs[k].0;
        for (a, b) in lit.iter_mut().zip(&directs[k].1) {
            *a |= *b;
        }
        if count > 0 {
            let (mut direct_k, mut indirect_k) = (1.0, 1.0);
            if w.ambient_occlusion {
                let frac = (occluded[k] as f32 / (w.fully_occluded_fraction.max(1e-3) * count as f32)).min(1.0);
                let occ = (0.5 * (w.occlusion_exponent + 1.0) * frac.powf(w.occlusion_exponent)).clamp(0.0, 1.0);
                direct_k = 1.0 - w.direct_occlusion * occ;
                indirect_k = 1.0 - w.indirect_occlusion * occ;
            }
            for c in 0..4 {
                for ch in 0..3 {
                    g.c[c][ch] = g.c[c][ch] * direct_k + indirect[k].c[c][ch] * indirect_k;
                }
            }
        }
        inner[i] = g;
    }
    rows_done.fetch_add(n - n / 2, Ordering::Relaxed);
    let mapped_count = inner.iter().filter(|g| g.mapped).count();
    let gathered = if padded { pad(&inner, n) } else { inner };
    let samples = gathered.iter().map(convert).collect();
    Baked { size, gathered, samples, lights: lit.iter().enumerate().filter(|(_, b)| **b).map(|(i, _)| i).collect(), mapped: mapped_count }
}
