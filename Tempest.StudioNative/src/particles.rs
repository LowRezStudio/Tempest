//! Cooked particle systems: the model decoded from `particle.system`
//! (baked distribution tables, module properties) and a CPU simulation of
//! the common sprite and mesh emitter modules, for the preview. A port of
//! the Electron Studio's `particles.ts`; positions, sizes and velocities are
//! in Unreal units and axes (Z up).

use std::collections::HashMap;

use serde_json::Value as J;

/// One `FRawDistribution*`: `[min, max]` then entries of `chunk` floats.
#[derive(Clone, Debug, PartialEq)]
pub struct Dist {
    /// Property name on the module (as the package spells it).
    pub prop: String,
    pub vector: bool,
    /// 0 none, 1 random (two values per entry), 2 extreme.
    pub op: u8,
    pub num_elements: usize,
    pub chunk: usize,
    /// Components per value (1 or 3).
    pub comps: usize,
    pub time_scale: f32,
    pub start_time: f32,
    pub entries: Vec<Vec<f32>>,
}

fn num(v: &J) -> Option<f32> {
    match v["t"].as_str()? {
        "float" | "int" | "byte" => v["v"].as_f64().map(|f| f as f32),
        "bool" => v["v"].as_bool().map(|b| if b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// The property `name` (array index 0) of a parsed object.
pub fn prop<'a>(obj: &'a J, name: &str) -> Option<&'a J> {
    obj["properties"].as_array()?.iter().find(|p| p["index"].as_u64() == Some(0) && p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

pub fn num_prop(obj: &J, name: &str, d: f32) -> f32 {
    prop(obj, name).and_then(|p| num(&p["value"])).unwrap_or(d)
}

pub fn bool_prop(obj: &J, name: &str, d: bool) -> bool {
    prop(obj, name).and_then(|p| (p["value"]["t"] == "bool").then(|| p["value"]["v"].as_bool()).flatten()).unwrap_or(d)
}

pub fn enum_prop(obj: &J, name: &str, d: &str) -> String {
    prop(obj, name).and_then(|p| matches!(p["value"]["t"].as_str(), Some("enum" | "name")).then(|| p["value"]["v"].as_str().map(|s| s.to_ascii_lowercase())).flatten()).unwrap_or_else(|| d.to_string())
}

pub fn ref_path(obj: &J, name: &str) -> Option<String> {
    let v = &prop(obj, name)?["value"];
    (v["t"] == "object" && v["v"]["ref"].as_i64().unwrap_or(0) != 0).then(|| v["v"]["path"].as_str().map(|s| s.to_string())).flatten()
}

fn member<'a>(members: &'a [J], name: &str) -> Option<&'a J> {
    members.iter().find(|m| m["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).map(|m| &m["value"])
}

/// Is this property a raw distribution struct?
pub fn is_dist(p: &J) -> bool {
    p["value"]["t"] == "struct" && p["struct"].as_str().is_some_and(|s| s.to_ascii_lowercase().starts_with("rawdistribution"))
}

/// Decode a raw distribution property of a module; `None` when it is not one.
pub fn dist_of(obj: &J, name: &str) -> Option<Dist> {
    let p = prop(obj, name)?;
    (p["value"]["t"] == "struct").then(|| decode(p))
}

/// Decode a property known to be a distribution struct.
pub fn decode(p: &J) -> Dist {
    let vector = p["struct"].as_str().is_some_and(|s| s.to_ascii_lowercase().contains("vector"));
    let members = p["value"]["v"].as_array().map(|a| a.as_slice()).unwrap_or_default();
    let get = |n: &str| member(members, n).and_then(num);
    let op = get("Op").unwrap_or(0.0) as u8;
    let comps0 = if vector { 3 } else { 1 };
    let num_elements = (get("LookupTableNumElements").unwrap_or(if op == 0 { 1.0 } else { 2.0 }) as usize).max(1);
    let chunk = match get("LookupTableChunkSize").unwrap_or(0.0) as usize {
        0 => comps0 * num_elements,
        c => c,
    };
    let comps = ((chunk as f32 / num_elements as f32).round() as usize).max(1);
    let table: Vec<f32> = match member(members, "LookupTable") {
        Some(t) if t["t"] == "array" => t["v"].as_array().map(|a| a.iter().map(|x| num(x).unwrap_or(0.0)).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    let entries = if table.len() > 2 { table[2..].chunks_exact(chunk).map(|c| c.to_vec()).collect() } else { Vec::new() };
    Dist { prop: p["name"].as_str().unwrap_or_default().to_string(), vector, op, num_elements, chunk, comps, time_scale: get("LookupTableTimeScale").unwrap_or(0.0), start_time: get("LookupTableStartTime").unwrap_or(0.0), entries }
}

impl Dist {
    /// The flat table (`[min, max, ...entries]`) as the package stores it.
    pub fn table(&self) -> Vec<f32> {
        let flat: Vec<f32> = self.entries.iter().flatten().copied().collect();
        let (min, max) = if flat.is_empty() { (0.0, 0.0) } else { flat.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v))) };
        let mut out = vec![min, max];
        out.extend(flat);
        out
    }

    /// Does it vary over time (more than one distinct entry)?
    pub fn is_curve(&self) -> bool {
        self.entries.len() >= 2 && self.entries.iter().any(|e| *e != self.entries[0])
    }

    /// Two values per entry (random between A and B, or one of them).
    pub fn ranged(&self) -> bool {
        self.num_elements >= 2
    }

    /// The seconds the entries span (0 for a constant).
    pub fn span(&self) -> f32 {
        if self.entries.len() > 1 && self.time_scale != 0.0 {
            (self.entries.len() - 1) as f32 / self.time_scale
        } else {
            0.0
        }
    }

    /// `FRawDistribution::GetValue`: the entry at `time`, lerped, one value per
    /// component; `rnd` picks between the two values of a random/extreme entry.
    pub fn sample(&self, time: f32, rnd: Option<[f32; 3]>) -> [f32; 3] {
        let mut out = [0.0; 3];
        let n = self.entries.len();
        if n == 0 {
            return out;
        }
        let mut t = (time - self.start_time) * self.time_scale;
        if !t.is_finite() {
            t = 0.0;
        }
        let t = t.clamp(0.0, (n - 1) as f32);
        let i = t.floor() as usize;
        let a = t - i as f32;
        let (e1, e2) = (&self.entries[i], &self.entries[(i + 1).min(n - 1)]);
        let at = |e: &[f32], k: usize| e.get(k).copied().unwrap_or(0.0);
        for (c, o) in out.iter_mut().enumerate().take(self.comps.min(3)) {
            let v1 = at(e1, c) + (at(e2, c) - at(e1, c)) * a;
            *o = if self.ranged() {
                let k = self.comps + c;
                let w1 = at(e1, k) + (at(e2, k) - at(e1, k)) * a;
                let r = rnd.map(|r| r[c]).unwrap_or(0.5);
                if self.op == 2 {
                    if r < 0.5 { v1 } else { w1 }
                } else {
                    v1 + (w1 - v1) * r
                }
            } else {
                v1
            };
        }
        out
    }

    /// Change the number of entries, keeping the curve's shape and time span.
    pub fn resample(&mut self, wanted: usize) {
        let count = wanted.clamp(2, 256);
        let n = self.entries.len();
        let zero = vec![0.0; self.chunk];
        let entries = (0..count)
            .map(|i| {
                let t = if n > 1 { i as f32 / (count - 1) as f32 * (n - 1) as f32 } else { 0.0 };
                let a = t.floor() as usize;
                let f = t - a as f32;
                let e1 = self.entries.get(a.min(n.saturating_sub(1))).unwrap_or(&zero);
                let e2 = self.entries.get((a + 1).min(n.saturating_sub(1))).unwrap_or(e1);
                e1.iter().zip(e2).map(|(v, w)| v + (w - v) * f).collect()
            })
            .collect();
        let span = if n > 1 && self.time_scale != 0.0 { (n - 1) as f32 / self.time_scale } else { 1.0 };
        self.entries = entries;
        self.time_scale = (count - 1) as f32 / span;
    }

    /// The name of value `k` of an entry: `X`, `AY`, `value`…
    pub fn label(&self, k: usize) -> String {
        let comp = if self.comps == 3 { ["X", "Y", "Z"][k % 3] } else { "" };
        if self.ranged() {
            format!("{}{comp}", if k < self.comps { "A" } else { "B" })
        } else if comp.is_empty() {
            "value".into()
        } else {
            comp.into()
        }
    }
}

// ----------------------------------------------------------------------------
// System model
// ----------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Burst {
    pub count: i32,
    pub count_low: i32,
    pub time: f32,
}

#[derive(Clone, Debug)]
pub struct Sphere {
    pub radius: Option<Dist>,
    pub surface: bool,
    pub velocity: bool,
    pub velocity_scale: Option<Dist>,
    pub location: Option<Dist>,
}

#[derive(Clone, Debug)]
pub struct Cylinder {
    pub radius: Option<Dist>,
    pub height: Option<Dist>,
    pub axis: String,
    pub surface: bool,
    pub velocity: bool,
    pub velocity_scale: Option<Dist>,
    pub location: Option<Dist>,
}

/// What the simulation needs from one emitter's LOD 0.
#[derive(Clone, Debug, Default)]
pub struct EmitterModel {
    pub index: usize,
    pub name: String,
    pub enabled: bool,
    pub material: Option<String>,
    pub mesh: Option<String>,
    /// Mesh emitters use the required module's material when set.
    pub override_material: bool,
    pub duration: f32,
    pub loops: i32,
    pub delay: f32,
    pub sub_h: f32,
    pub sub_v: f32,
    /// `none`, `linear`, `random`…
    pub sub_method: String,
    pub axis_lock: String,
    pub rate: Option<Dist>,
    pub rate_scale: Option<Dist>,
    pub bursts: Vec<Burst>,
    pub lifetime: Option<Dist>,
    pub size: Option<Dist>,
    pub size_multiply_life: Option<Dist>,
    pub size_multiply_mask: [bool; 3],
    pub size_scale: Option<Dist>,
    pub velocity: Option<Dist>,
    pub velocity_radial: Option<Dist>,
    pub velocity_over_life: Option<Dist>,
    pub velocity_over_life_absolute: bool,
    pub acceleration: Option<Dist>,
    pub color: Option<Dist>,
    pub alpha: Option<Dist>,
    pub color_over_life: Option<Dist>,
    pub alpha_over_life: Option<Dist>,
    pub color_scale_over_life: Option<Dist>,
    pub alpha_scale_over_life: Option<Dist>,
    pub rotation: Option<Dist>,
    pub rotation_rate: Option<Dist>,
    pub sub_image_index: Option<Dist>,
    pub locations: Vec<Dist>,
    pub sphere: Option<Sphere>,
    pub cylinder: Option<Cylinder>,
    pub orbit_offset: Option<Dist>,
    pub orbit_rotation: Option<Dist>,
    pub orbit_rotation_rate: Option<Dist>,
    pub mesh_rotation: Option<Dist>,
    pub mesh_rotation_rate: Option<Dist>,
    /// Modules the simulation does not interpret.
    pub unsupported: Vec<String>,
    pub peak: usize,
}

const SUPPORTED: &[&str] = &[
    "particlemodulelifetime",
    "particlemodulesize",
    "particlemodulesizemultiplylife",
    "particlemodulesizescale",
    "particlemodulevelocity",
    "particlemodulevelocityoverlifetime",
    "particlemoduleacceleration",
    "particlemodulecolor",
    "particlemodulecoloroverlife",
    "particlemodulecolorscaleoverlife",
    "particlemodulerotation",
    "particlemodulerotationrate",
    "particlemodulesubuv",
    "particlemodulelocation",
    "particlemodulelocationprimitivesphere",
    "particlemodulelocationprimitivecylinder",
    "particlemoduleorbit",
    "particlemodulemeshrotation",
    "particlemodulemeshrotationrate",
    "particlemoduleorientationaxislock",
    "particlemodulecameraoffset",
    "particlemoduleparameterdynamic",
    "particlemodulevelocityinheritparent",
    "particlemodulespawn",
    "particlemodulerequired",
    "particlemoduletypedatamesh",
];

/// Distributions being edited, by (module export, property): they replace
/// what the system JSON stores, so the preview follows a curve as it is dragged.
pub type Drafts = HashMap<(usize, String), Dist>;

/// The modules of an emitter's LOD 0 with their role: required, spawn, type data, then the rest.
pub fn modules(e: &J) -> Vec<(&J, &'static str)> {
    let lod = &e["lods"][0];
    let mut out = Vec::new();
    for (k, role) in [("required", "required"), ("spawn", "spawn"), ("typeData", "type data")] {
        if lod[k].is_object() {
            out.push((&lod[k], role));
        }
    }
    for m in lod["modules"].as_array().map(|a| a.as_slice()).unwrap_or_default() {
        out.push((m, ""));
    }
    out
}

/// Every distribution of a module, drafts in place of the stored ones.
pub fn module_dists(m: &J, drafts: &Drafts) -> Vec<Dist> {
    let index = m["index"].as_u64().unwrap_or(0) as usize;
    m["properties"].as_array().map(|a| a.as_slice()).unwrap_or_default().iter().filter(|p| is_dist(p)).map(|p| drafts.get(&(index, p["name"].as_str().unwrap_or_default().to_string())).cloned().unwrap_or_else(|| decode(p))).collect()
}

pub fn build_emitter(e: &J, drafts: &Drafts) -> EmitterModel {
    let lod = &e["lods"][0];
    let (req, spawn, td) = (&lod["required"], &lod["spawn"], &lod["typeData"]);
    let dist = |obj: &J, name: &str| -> Option<Dist> {
        let p = prop(obj, name)?;
        let key = (obj["index"].as_u64().unwrap_or(0) as usize, p["name"].as_str().unwrap_or_default().to_string());
        drafts.get(&key).cloned().or_else(|| dist_of(obj, name))
    };
    let td_class = td["class"].as_str().unwrap_or_default().to_ascii_lowercase();
    let name = match prop(&e["props"], "EmitterName").map(|p| &p["value"]) {
        Some(v) if v["t"] == "name" => v["v"].as_str().unwrap_or_default().to_string(),
        _ => e["path"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default().to_string(),
    };
    let mut m = EmitterModel {
        index: e["index"].as_u64().unwrap_or(0) as usize,
        name,
        enabled: bool_prop(&lod["props"], "bEnabled", true) && enum_prop(&e["props"], "EmitterRenderMode", "erm_normal") != "erm_none",
        material: ref_path(req, "Material"),
        mesh: (td_class == "particlemoduletypedatamesh").then(|| ref_path(td, "Mesh")).flatten(),
        override_material: bool_prop(td, "bOverrideMaterial", false),
        duration: num_prop(req, "EmitterDuration", 1.0),
        loops: num_prop(req, "EmitterLoops", 0.0) as i32,
        delay: num_prop(req, "EmitterDelay", 0.0),
        sub_h: num_prop(req, "SubImages_Horizontal", 1.0).max(1.0),
        sub_v: num_prop(req, "SubImages_Vertical", 1.0).max(1.0),
        sub_method: enum_prop(req, "InterpolationMethod", "psuvim_none").trim_start_matches("psuvim_").to_string(),
        rate: dist(spawn, "Rate"),
        rate_scale: dist(spawn, "RateScale"),
        size_multiply_mask: [true; 3],
        peak: num_prop(&lod["props"], "PeakActiveParticles", 64.0).max(0.0) as usize,
        ..Default::default()
    };
    if let Some(list) = prop(spawn, "BurstList").filter(|p| p["value"]["t"] == "array").and_then(|p| p["value"]["v"].as_array()) {
        for b in list.iter().filter(|b| b["t"] == "struct") {
            let members = b["v"].as_array().map(|a| a.as_slice()).unwrap_or_default();
            let get = |n: &str| member(members, n).and_then(num);
            m.bursts.push(Burst { count: get("Count").unwrap_or(0.0) as i32, count_low: get("CountLow").unwrap_or(-1.0) as i32, time: get("Time").unwrap_or(0.0) });
        }
    }
    if td.is_object() && !SUPPORTED.contains(&td_class.as_str()) {
        m.unsupported.push(td["class"].as_str().unwrap_or_default().to_string());
    }
    for module in lod["modules"].as_array().map(|a| a.as_slice()).unwrap_or_default() {
        if !bool_prop(module, "bEnabled", true) {
            continue;
        }
        let c = module["class"].as_str().unwrap_or_default().to_ascii_lowercase();
        let d = |n: &str| dist(module, n);
        match c.as_str() {
            "particlemodulelifetime" => m.lifetime = d("Lifetime"),
            "particlemodulesize" => m.size = d("StartSize"),
            "particlemodulesizemultiplylife" => {
                m.size_multiply_life = d("LifeMultiplier");
                m.size_multiply_mask = [bool_prop(module, "MultiplyX", true), bool_prop(module, "MultiplyY", true), bool_prop(module, "MultiplyZ", true)];
            }
            "particlemodulesizescale" => m.size_scale = d("SizeScale"),
            "particlemodulevelocity" => {
                m.velocity = d("StartVelocity");
                m.velocity_radial = d("StartVelocityRadial");
            }
            "particlemodulevelocityoverlifetime" => {
                m.velocity_over_life = d("VelOverLife");
                m.velocity_over_life_absolute = bool_prop(module, "Absolute", false);
            }
            "particlemoduleacceleration" => m.acceleration = d("Acceleration"),
            "particlemodulecolor" => {
                m.color = d("StartColor");
                m.alpha = d("StartAlpha");
            }
            "particlemodulecoloroverlife" => {
                m.color_over_life = d("ColorOverLife");
                m.alpha_over_life = d("AlphaOverLife");
            }
            "particlemodulecolorscaleoverlife" => {
                m.color_scale_over_life = d("ColorScaleOverLife");
                m.alpha_scale_over_life = d("AlphaScaleOverLife");
            }
            "particlemodulerotation" => m.rotation = d("StartRotation"),
            "particlemodulerotationrate" => m.rotation_rate = d("StartRotationRate"),
            "particlemodulesubuv" => m.sub_image_index = d("SubImageIndex"),
            "particlemodulelocation" => m.locations.extend(d("StartLocation")),
            "particlemodulelocationprimitivesphere" => m.sphere = Some(Sphere { radius: d("StartRadius"), surface: bool_prop(module, "SurfaceOnly", false), velocity: bool_prop(module, "Velocity", false), velocity_scale: d("VelocityScale"), location: d("StartLocation") }),
            "particlemodulelocationprimitivecylinder" => {
                m.cylinder = Some(Cylinder { radius: d("StartRadius"), height: d("StartHeight"), axis: enum_prop(module, "HeightAxis", "psphc_z"), surface: bool_prop(module, "SurfaceOnly", false), velocity: bool_prop(module, "Velocity", false), velocity_scale: d("VelocityScale"), location: d("StartLocation") })
            }
            "particlemoduleorbit" => {
                m.orbit_offset = d("OffsetAmount");
                m.orbit_rotation = d("RotationAmount");
                m.orbit_rotation_rate = d("RotationRateAmount");
            }
            "particlemodulemeshrotation" => m.mesh_rotation = d("StartRotation"),
            "particlemodulemeshrotationrate" => m.mesh_rotation_rate = d("StartRotationRate"),
            "particlemoduleorientationaxislock" => m.axis_lock = enum_prop(module, "LockAxisFlags", ""),
            _ if !SUPPORTED.contains(&c.as_str()) => m.unsupported.push(module["class"].as_str().unwrap_or_default().to_string()),
            _ => {}
        }
    }
    m
}

impl EmitterModel {
    /// How many particles the views hold at most.
    pub fn cap(&self) -> usize {
        (self.peak * 4).max(256)
    }
}

// ----------------------------------------------------------------------------
// Simulation
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Particle {
    pub age: f32,
    pub life: f32,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub base_size: [f32; 3],
    pub size: [f32; 3],
    pub rot: f32,
    pub rot_rate: f32,
    pub base_color: [f32; 4],
    pub color: [f32; 4],
    pub sub: f32,
    pub sub_rnd: f32,
    pub orbit: [f32; 3],
    pub orbit_rot: [f32; 3],
    pub orbit_rate: [f32; 3],
    pub mesh_rot: [f32; 3],
    pub mesh_rate: [f32; 3],
    pub accel: [f32; 3],
}

const TAU: f32 = std::f32::consts::TAU;

pub struct EmitterSim {
    pub model: EmitterModel,
    pub particles: Vec<Particle>,
    pub time: f32,
    pub finished: bool,
    loops_done: i64,
    spawn_accum: f32,
    bursts_done: Vec<bool>,
    rng: Mulberry,
}

impl EmitterSim {
    pub fn new(model: EmitterModel, seed: u32) -> EmitterSim {
        EmitterSim { model, particles: Vec::new(), time: 0.0, finished: false, loops_done: 0, spawn_accum: 0.0, bursts_done: Vec::new(), rng: Mulberry(seed.max(1)) }
    }

    pub fn reset(&mut self, seed: u32) {
        *self = EmitterSim::new(std::mem::take(&mut self.model), seed);
    }

    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        let m = &self.model;
        let et = (self.time - m.delay).max(0.0);
        let duration = m.duration.max(0.001);
        let lp = (et / duration).floor() as i64;
        if m.loops > 0 && lp >= m.loops as i64 {
            self.finished = true;
        } else if lp > self.loops_done {
            self.loops_done = lp;
            self.bursts_done.clear();
        }
        let local = et - lp as f32 * duration;
        if !self.finished && self.time >= m.delay && m.enabled {
            let mut rate = m.rate.as_ref().map(|d| d.sample(local, None)[0]).unwrap_or(0.0);
            if let Some(s) = &m.rate_scale {
                rate *= s.sample(local, None)[0];
            }
            self.spawn_accum += rate.max(0.0) * dt;
            let mut count = self.spawn_accum.floor();
            self.spawn_accum -= count;
            self.bursts_done.resize(m.bursts.len(), false);
            for k in 0..m.bursts.len() {
                let b = &self.model.bursts[k];
                if !self.bursts_done[k] && local >= b.time * duration {
                    self.bursts_done[k] = true;
                    let n = if b.count_low >= 0 { b.count_low + (self.rng.next() * (b.count - b.count_low + 1) as f32).floor() as i32 } else { b.count };
                    count += n.max(0) as f32;
                }
            }
            let cap = self.model.cap();
            for _ in 0..count as usize {
                if self.particles.len() >= cap {
                    break;
                }
                self.spawn(local);
            }
        }
        let m = &self.model;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            let rel = p.age / p.life;
            let mut v = p.vel;
            if let Some(d) = &m.velocity_over_life {
                let s = d.sample(rel, None);
                if m.velocity_over_life_absolute {
                    v = s;
                } else {
                    (0..3).for_each(|c| v[c] *= s[c]);
                }
            }
            for (c, vc) in v.iter().enumerate() {
                p.pos[c] += vc * dt;
                p.vel[c] += p.accel[c] * dt;
                p.mesh_rot[c] += p.mesh_rate[c] * dt;
                p.orbit_rot[c] += p.orbit_rate[c] * dt;
            }
            p.rot += p.rot_rate * dt;
            p.size = p.base_size;
            if let Some(d) = &m.size_multiply_life {
                let s = d.sample(rel, None);
                (0..3).filter(|&c| m.size_multiply_mask[c]).for_each(|c| p.size[c] *= s[c]);
            }
            if let Some(d) = &m.size_scale {
                let s = d.sample(rel, None);
                (0..3).for_each(|c| p.size[c] *= s[c]);
            }
            p.color = p.base_color;
            if let Some(d) = &m.color_over_life {
                let s = d.sample(rel, None);
                p.color[..3].copy_from_slice(&s);
            }
            if let Some(d) = &m.alpha_over_life {
                p.color[3] = d.sample(rel, None)[0];
            }
            if let Some(d) = &m.color_scale_over_life {
                let s = d.sample(rel, None);
                (0..3).for_each(|c| p.color[c] *= s[c]);
            }
            if let Some(d) = &m.alpha_scale_over_life {
                p.color[3] *= d.sample(rel, None)[0];
            }
            if let Some(d) = &m.sub_image_index {
                p.sub = d.sample(rel, None)[0];
            } else if m.sub_method.starts_with("random") {
                p.sub = p.sub_rnd;
            } else if m.sub_method != "none" {
                p.sub = rel * m.sub_h * m.sub_v;
            }
            true
        });
    }

    fn rnd3(&mut self) -> Option<[f32; 3]> {
        Some([self.rng.next(), self.rng.next(), self.rng.next()])
    }

    fn spawn(&mut self, et: f32) {
        let m = self.model.clone();
        let life = match &m.lifetime {
            Some(d) => {
                let r = self.rnd3();
                d.sample(et, r)[0].max(0.01)
            }
            None => 1.0,
        };
        let mut p = Particle { life, base_size: [1.0; 3], base_color: [1.0; 4], sub_rnd: (self.rng.next() * m.sub_h * m.sub_v).floor(), ..Default::default() };
        let add = |a: &mut [f32; 3], b: [f32; 3]| (0..3).for_each(|c| a[c] += b[c]);
        for loc in &m.locations {
            let r = self.rnd3();
            add(&mut p.pos, loc.sample(et, r));
        }
        if let Some(s) = &m.sphere {
            let radius = match &s.radius {
                Some(d) => {
                    let r = self.rnd3();
                    d.sample(et, r)[0]
                }
                None => 0.0,
            };
            let u = self.rng.next() * TAU;
            let v = (2.0 * self.rng.next() - 1.0).acos();
            let d = if s.surface { radius } else { radius * self.rng.next().cbrt() };
            let dir = [v.sin() * u.cos(), v.sin() * u.sin(), v.cos()];
            add(&mut p.pos, dir.map(|x| x * d));
            if let Some(l) = &s.location {
                let r = self.rnd3();
                add(&mut p.pos, l.sample(et, r));
            }
            if s.velocity {
                let vs = match &s.velocity_scale {
                    Some(d) => {
                        let r = self.rnd3();
                        d.sample(et, r)[0]
                    }
                    None => 1.0,
                };
                add(&mut p.vel, dir.map(|x| x * d * vs));
            }
        }
        if let Some(c) = &m.cylinder {
            let sample1 = |d: &Option<Dist>, def: f32, this: &mut Self| match d {
                Some(d) => {
                    let r = this.rnd3();
                    d.sample(et, r)[0]
                }
                None => def,
            };
            let radius = sample1(&c.radius, 0.0, self);
            let height = sample1(&c.height, 0.0, self);
            let u = self.rng.next() * TAU;
            let d = if c.surface { radius } else { radius * self.rng.next().sqrt() };
            let along = (self.rng.next() - 0.5) * height;
            let (radial, off) = if c.axis.ends_with("_x") {
                let r = [0.0, u.cos(), u.sin()];
                (r, [along, r[1] * d, r[2] * d])
            } else if c.axis.ends_with("_y") {
                let r = [u.cos(), 0.0, u.sin()];
                (r, [r[0] * d, along, r[2] * d])
            } else {
                let r = [u.cos(), u.sin(), 0.0];
                (r, [r[0] * d, r[1] * d, along])
            };
            add(&mut p.pos, off);
            if let Some(l) = &c.location {
                let r = self.rnd3();
                add(&mut p.pos, l.sample(et, r));
            }
            if c.velocity {
                let vs = sample1(&c.velocity_scale, 1.0, self);
                add(&mut p.vel, radial.map(|x| x * d * vs));
            }
        }
        let mut s3 = |d: &Option<Dist>| {
            d.as_ref().map(|d| {
                let r = self.rnd3();
                d.sample(et, r)
            })
        };
        if let Some(v) = s3(&m.velocity) {
            add(&mut p.vel, v);
        }
        if let Some(vr) = s3(&m.velocity_radial).map(|v| v[0]) {
            let len = (p.pos[0].powi(2) + p.pos[1].powi(2) + p.pos[2].powi(2)).sqrt();
            if len > 1e-6 && vr != 0.0 {
                add(&mut p.vel, p.pos.map(|x| x / len * vr));
            }
        }
        if let Some(a) = s3(&m.acceleration) {
            p.accel = a;
        }
        if let Some(s) = s3(&m.size) {
            p.base_size = s;
        }
        if let Some(c) = s3(&m.color) {
            p.base_color[..3].copy_from_slice(&c);
        }
        if let Some(a) = s3(&m.alpha) {
            p.base_color[3] = a[0];
        }
        if let Some(r) = s3(&m.rotation) {
            p.rot = r[0] * TAU;
        }
        if let Some(r) = s3(&m.rotation_rate) {
            p.rot_rate = r[0] * TAU;
        }
        if let Some(o) = s3(&m.orbit_offset) {
            p.orbit = o;
        }
        if let Some(o) = s3(&m.orbit_rotation) {
            p.orbit_rot = o.map(|x| x * TAU);
        }
        if let Some(o) = s3(&m.orbit_rotation_rate) {
            p.orbit_rate = o.map(|x| x * TAU);
        }
        if let Some(r) = s3(&m.mesh_rotation) {
            p.mesh_rot = r.map(|x| x * TAU);
        }
        if let Some(r) = s3(&m.mesh_rotation_rate) {
            p.mesh_rate = r.map(|x| x * TAU);
        }
        p.size = p.base_size;
        p.color = p.base_color;
        self.particles.push(p);
    }
}

/// A small deterministic PRNG so the preview replays the same way (mulberry32).
struct Mulberry(u32);

impl Mulberry {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f32 / 4_294_967_296.0
    }
}

/// The orbit offset of a particle (rotated around the particle's own origin: Z, then Y, then X).
pub fn orbit_offset(p: &Particle) -> [f32; 3] {
    let [mut x, mut y, mut z] = p.orbit;
    let [rx, ry, rz] = p.orbit_rot;
    let (s, c) = rz.sin_cos();
    (x, y) = (x * c - y * s, x * s + y * c);
    let (s, c) = ry.sin_cos();
    (x, z) = (x * c + z * s, -x * s + z * c);
    let (s, c) = rx.sin_cos();
    (y, z) = (y * c - z * s, y * s + z * c);
    [x, y, z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn f(v: f32) -> J {
        json!({ "t": "float", "v": v })
    }

    fn dist_prop(name: &str, strukt: &str, op: u8, table: &[f32], time_scale: f32) -> J {
        json!({ "name": name, "index": 0, "type": "StructProperty", "struct": strukt, "value": { "t": "struct", "v": [
            { "name": "Op", "index": 0, "value": { "t": "byte", "v": op } },
            { "name": "LookupTableTimeScale", "index": 0, "value": f(time_scale) },
            { "name": "LookupTable", "index": 0, "value": { "t": "array", "v": table.iter().map(|x| f(*x)).collect::<Vec<_>>() } },
        ] } })
    }

    #[test]
    fn decodes_and_samples() {
        // A float curve 0 → 10 over two entries, one per second.
        let d = decode(&dist_prop("Lifetime", "RawDistributionFloat", 0, &[0.0, 10.0, 0.0, 10.0], 1.0));
        assert_eq!((d.comps, d.chunk, d.entries.len()), (1, 1, 2));
        assert_eq!(d.sample(0.5, None)[0], 5.0);
        assert_eq!(d.sample(9.0, None)[0], 10.0);
        assert_eq!(d.table(), vec![0.0, 10.0, 0.0, 10.0]);
        // A random vector: A then B per entry, picked by `rnd`.
        let v = decode(&dist_prop("StartVelocity", "RawDistributionVector", 1, &[-1.0, 1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0], 0.0));
        assert_eq!((v.comps, v.chunk, v.num_elements, v.entries.len()), (3, 6, 2, 1));
        assert_eq!(v.sample(0.0, Some([0.0, 0.5, 1.0])), [-1.0, 0.0, 1.0]);
        assert_eq!(v.label(4), "BY");
    }

    #[test]
    fn resample_keeps_span() {
        let mut d = decode(&dist_prop("Size", "RawDistributionFloat", 0, &[0.0, 4.0, 0.0, 4.0], 0.5));
        assert_eq!(d.span(), 2.0);
        d.resample(5);
        assert_eq!(d.entries.len(), 5);
        assert!((d.span() - 2.0).abs() < 1e-6);
        assert_eq!(d.entries.iter().map(|e| e[0]).collect::<Vec<_>>(), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn spawns_and_moves() {
        let module = |class: &str, props: Vec<J>| json!({ "index": 9, "class": class, "properties": props });
        let e = json!({ "index": 3, "path": "P.E", "props": { "properties": [] }, "lods": [{
            "props": { "properties": [] },
            "required": { "index": 4, "class": "ParticleModuleRequired", "properties": [] },
            "spawn": { "index": 5, "class": "ParticleModuleSpawn", "properties": [dist_prop("Rate", "RawDistributionFloat", 0, &[100.0, 100.0, 100.0], 0.0)] },
            "typeData": null,
            "modules": [
                module("ParticleModuleLifetime", vec![dist_prop("Lifetime", "RawDistributionFloat", 0, &[2.0, 2.0, 2.0], 0.0)]),
                module("ParticleModuleVelocity", vec![dist_prop("StartVelocity", "RawDistributionVector", 0, &[0.0, 50.0, 0.0, 0.0, 50.0], 0.0)]),
                module("ParticleModuleBeamNoise", vec![]),
            ],
        }] });
        let model = build_emitter(&e, &Drafts::new());
        assert_eq!(model.unsupported, vec!["ParticleModuleBeamNoise".to_string()]);
        let mut sim = EmitterSim::new(model, 1);
        for _ in 0..60 {
            sim.step(1.0 / 60.0);
        }
        // 100 per second for one second, none old enough to die.
        assert!((95..=100).contains(&sim.particles.len()), "{}", sim.particles.len());
        let top = sim.particles.iter().map(|p| p.pos[2]).fold(0.0, f32::max);
        assert!(top > 45.0 && top <= 50.0, "{top}");
        // A draft replaces the stored rate.
        let mut drafts = Drafts::new();
        let mut rate = sim.model.rate.clone().unwrap();
        rate.entries = vec![vec![0.0]];
        drafts.insert((5, "Rate".into()), rate);
        assert_eq!(build_emitter(&e, &drafts).rate.unwrap().entries, vec![vec![0.0]]);
    }

    #[test]
    fn mulberry_matches_the_electron_preview() {
        // mulberry(1)'s first values in JS.
        let mut r = Mulberry(1);
        let a = r.next();
        assert!((a - 0.627_073_9).abs() < 1e-6, "{a}");
    }
}
