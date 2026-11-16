//! The game's compiled shaders: `UShaderCache::Load` / `FShaderCache::Load`
//! / `SerializeShaders` / `FMaterialShaderMap::Serialize` /
//! `FUniformExpressionSet::Serialize` (ShaderCache.cpp, MaterialShared.cpp,
//! MaterialUniformExpressions.h) as the 8.2 binary has them, which is the
//! 10897 source at file version 893, and Hi-Rez's `FShaderCodeStorageHirez`
//! (`ShaderCodeStorage-PC-D3D-SM3.bin`): the bytecode of every shader is
//! kept out of the packages, in one file per platform, by shader id.
//!
//! A package's `ShaderCache` export lists its shaders (type and id; the
//! per-type parameter bindings are skipped: the D3D9 bytecode names its
//! constants in its `CTAB`) and one `FMaterialShaderMap` per material and
//! static parameter set: which shader of each type draws it with each
//! vertex factory, and the uniform expressions the engine evaluates on the
//! CPU into the shaders' `Uniform*` constants.
//!
//! Read only (the cache is kept as raw bytes when a package is edited).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempest_upk::io::{Error, FName, Guid, Reader, Result};
use tempest_upk::package::read_compressed_chunk;

use crate::material::StaticParameterSet;

// ----- the code storage --------------------------------------------------------------

/// `ShaderCodeStorage-<platform>.bin`: `version, platform, chunk size, count`,
/// an index of `(shader id, chunk, offset in the chunk, size)`, the chunks'
/// `(file offset, stored size)`, then the chunks (LZO, the package chunk
/// layout). A shader's code is a slice of its expanded chunk.
pub struct CodeStorage {
    path: PathBuf,
    data: Vec<u8>,
    index: HashMap<Guid, (u32, u32, u32)>,
    chunks: Vec<(u32, u32)>,
    /// The chunks expanded lately (a material's shaders sit in a few of them).
    expanded: Mutex<Vec<(u32, Arc<Vec<u8>>)>>,
}

impl CodeStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<CodeStorage> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path)?;
        let mut r = Reader::new(&data);
        let version = r.u32()?;
        if version != 2 {
            return Err(Error::Format(format!("{}: shader code storage version {version}, expected 2", path.display())));
        }
        let _platform = r.u32()?;
        let _chunk_size = r.u32()?;
        let count = r.u32()? as usize;
        let mut index = HashMap::with_capacity(count);
        for _ in 0..count {
            let id = r.guid()?;
            index.insert(id, (r.u32()?, r.u32()?, r.u32()?));
        }
        let chunks = r.array(|r| Ok((r.u32()?, r.u32()?)))?;
        Ok(CodeStorage { path, data, index, chunks, expanded: Mutex::new(Vec::new()) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains(&self, id: &Guid) -> bool {
        self.index.contains_key(id)
    }

    fn chunk(&self, k: u32) -> Result<Arc<Vec<u8>>> {
        if let Some(c) = self.expanded.lock().ok().and_then(|e| e.iter().find(|(i, _)| *i == k).map(|(_, c)| c.clone())) {
            return Ok(c);
        }
        let (offset, size) = *self.chunks.get(k as usize).ok_or_else(|| Error::Format(format!("shader code chunk {k} is out of range")))?;
        let stored = self.data.get(offset as usize..offset as usize + size as usize).ok_or_else(|| Error::Format(format!("shader code chunk {k} is past the end of the file")))?;
        let chunk = Arc::new(read_compressed_chunk(&mut Reader::new(stored), false)?);
        if let Ok(mut e) = self.expanded.lock() {
            if e.len() >= 24 {
                e.remove(0);
            }
            e.push((k, chunk.clone()));
        }
        Ok(chunk)
    }

    /// The bytecode of shader `id`, when the file has it.
    pub fn code(&self, id: &Guid) -> Result<Option<Vec<u8>>> {
        let Some(&(chunk, offset, size)) = self.index.get(id) else { return Ok(None) };
        let c = self.chunk(chunk)?;
        let code = c.get(offset as usize..offset as usize + size as usize).ok_or_else(|| Error::Format(format!("shader {id} is past the end of its chunk")))?;
        Ok(Some(code.to_vec()))
    }
}

/// The platform's storage files of a cooked folder: the patch file first (it overrides), then the base one.
pub fn open_storages(cooked_dir: &Path, platform: &str) -> Vec<CodeStorage> {
    [format!("ShaderCodeStorage-{platform}-PATCH.bin"), format!("ShaderCodeStorage-{platform}.bin")].iter().filter_map(|f| CodeStorage::open(cooked_dir.join(f)).ok()).collect()
}

// ----- uniform expressions ---------------------------------------------------------------

/// `FMaterialUniformExpression` and its subclasses: what the engine computes
/// per frame for a material's `Uniform*` shader constants.
#[derive(Clone, Debug, PartialEq)]
pub enum UniformExpr {
    Constant { value: [f32; 4], value_type: u8 },
    VectorParameter { name: String, default: [f32; 4] },
    ScalarParameter { name: String, default: f32 },
    /// Index into the material's `UniformExpressionTextures`.
    Texture { index: i32 },
    TextureParameter { name: String, index: i32 },
    FlipBookTextureParameter { index: i32 },
    Time,
    RealTime,
    Sine { x: Box<UniformExpr>, cosine: bool },
    SquareRoot(Box<UniformExpr>),
    Length(Box<UniformExpr>),
    Periodic(Box<UniformExpr>),
    Abs(Box<UniformExpr>),
    Floor(Box<UniformExpr>),
    Ceil(Box<UniformExpr>),
    Frac(Box<UniformExpr>),
    /// `op`: 0 add, 1 subtract, 2 multiply, 3 divide, 4 dot (`EFoldedMathOperation`).
    FoldedMath { a: Box<UniformExpr>, b: Box<UniformExpr>, op: u8 },
    AppendVector { a: Box<UniformExpr>, b: Box<UniformExpr>, components_a: i32 },
    Min(Box<UniformExpr>, Box<UniformExpr>),
    Max(Box<UniformExpr>, Box<UniformExpr>),
    Fmod(Box<UniformExpr>, Box<UniformExpr>),
    Clamp { input: Box<UniformExpr>, min: Box<UniformExpr>, max: Box<UniformExpr> },
}

/// What a material instance sets and the clock: the inputs of [`UniformExpr::eval`].
pub trait UniformInputs {
    fn scalar(&self, name: &str) -> Option<f32>;
    fn vector(&self, name: &str) -> Option<[f32; 4]>;
    /// Game time and real time, in seconds.
    fn time(&self) -> (f32, f32);
}

impl UniformExpr {
    fn read(r: &mut Reader, names: &dyn Fn(FName) -> String) -> Result<UniformExpr> {
        let ty = names(r.fname()?);
        let sub = |r: &mut Reader| -> Result<Box<UniformExpr>> { Ok(Box::new(UniformExpr::read(r, names)?)) };
        let color = |r: &mut Reader| -> Result<[f32; 4]> { Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?]) };
        Ok(match ty.to_ascii_lowercase().as_str() {
            "fmaterialuniformexpressionconstant" => UniformExpr::Constant { value: color(r)?, value_type: r.u8()? },
            "fmaterialuniformexpressionvectorparameter" => UniformExpr::VectorParameter { name: names(r.fname()?), default: color(r)? },
            "fmaterialuniformexpressionscalarparameter" => UniformExpr::ScalarParameter { name: names(r.fname()?), default: r.f32()? },
            "fmaterialuniformexpressiontexture" => UniformExpr::Texture { index: r.i32()? },
            "fmaterialuniformexpressiontextureparameter" => UniformExpr::TextureParameter { name: names(r.fname()?), index: r.i32()? },
            "fmaterialuniformexpressionflipbooktextureparameter" => UniformExpr::FlipBookTextureParameter { index: r.i32()? },
            "fmaterialuniformexpressiontime" => UniformExpr::Time,
            "fmaterialuniformexpressionrealtime" => UniformExpr::RealTime,
            "fmaterialuniformexpressionsine" => UniformExpr::Sine { x: sub(r)?, cosine: r.i32()? != 0 },
            "fmaterialuniformexpressionsquareroot" => UniformExpr::SquareRoot(sub(r)?),
            "fmaterialuniformexpressionlength" => UniformExpr::Length(sub(r)?),
            "fmaterialuniformexpressionperiodic" => UniformExpr::Periodic(sub(r)?),
            "fmaterialuniformexpressionabs" => UniformExpr::Abs(sub(r)?),
            "fmaterialuniformexpressionfloor" => UniformExpr::Floor(sub(r)?),
            "fmaterialuniformexpressionceil" => UniformExpr::Ceil(sub(r)?),
            "fmaterialuniformexpressionfrac" => UniformExpr::Frac(sub(r)?),
            "fmaterialuniformexpressionfoldedmath" => UniformExpr::FoldedMath { a: sub(r)?, b: sub(r)?, op: r.u8()? },
            "fmaterialuniformexpressionappendvector" => UniformExpr::AppendVector { a: sub(r)?, b: sub(r)?, components_a: r.i32()? },
            "fmaterialuniformexpressionmin" => UniformExpr::Min(sub(r)?, sub(r)?),
            "fmaterialuniformexpressionmax" => UniformExpr::Max(sub(r)?, sub(r)?),
            "fmaterialuniformexpressionfmod" => UniformExpr::Fmod(sub(r)?, sub(r)?),
            "fmaterialuniformexpressionclamp" => UniformExpr::Clamp { input: sub(r)?, min: sub(r)?, max: sub(r)? },
            other => return Err(Error::Format(format!("unknown uniform expression {other}"))),
        })
    }

    /// The expression's value (`GetNumberValue`): scalars fill the red channel like the engine's.
    pub fn eval(&self, inputs: &dyn UniformInputs) -> [f32; 4] {
        let each = |a: [f32; 4], f: &dyn Fn(f32) -> f32| [f(a[0]), f(a[1]), f(a[2]), f(a[3])];
        let both = |a: [f32; 4], b: [f32; 4], f: &dyn Fn(f32, f32) -> f32| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), f(a[3], b[3])];
        match self {
            UniformExpr::Constant { value, .. } => *value,
            UniformExpr::VectorParameter { name, default } => inputs.vector(name).unwrap_or(*default),
            UniformExpr::ScalarParameter { name, default } => {
                let v = inputs.scalar(name).unwrap_or(*default);
                [v, 0.0, 0.0, 0.0]
            }
            UniformExpr::Texture { .. } | UniformExpr::TextureParameter { .. } | UniformExpr::FlipBookTextureParameter { .. } => [0.0; 4],
            UniformExpr::Time => [inputs.time().0, 0.0, 0.0, 0.0],
            UniformExpr::RealTime => [inputs.time().1, 0.0, 0.0, 0.0],
            UniformExpr::Sine { x, cosine } => {
                let v = x.eval(inputs)[0];
                [if *cosine { v.cos() } else { v.sin() }, 0.0, 0.0, 0.0]
            }
            UniformExpr::SquareRoot(x) => [x.eval(inputs)[0].max(0.0).sqrt(), 0.0, 0.0, 0.0],
            UniformExpr::Length(x) => {
                let v = x.eval(inputs);
                [(v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]).sqrt(), 0.0, 0.0, 0.0]
            }
            UniformExpr::Periodic(x) => each(x.eval(inputs), &|v| v - v.floor()),
            UniformExpr::Abs(x) => each(x.eval(inputs), &f32::abs),
            UniformExpr::Floor(x) => each(x.eval(inputs), &f32::floor),
            UniformExpr::Ceil(x) => each(x.eval(inputs), &f32::ceil),
            UniformExpr::Frac(x) => each(x.eval(inputs), &|v| v - v.floor()),
            UniformExpr::FoldedMath { a, b, op } => {
                let (a, b) = (a.eval(inputs), b.eval(inputs));
                match op {
                    0 => both(a, b, &|x, y| x + y),
                    1 => both(a, b, &|x, y| x - y),
                    2 => both(a, b, &|x, y| x * y),
                    3 => both(a, b, &|x, y| x / y),
                    _ => [a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3], 0.0, 0.0, 0.0],
                }
            }
            UniformExpr::AppendVector { a, b, components_a } => {
                let (a, b) = (a.eval(inputs), b.eval(inputs));
                let n = (*components_a).clamp(0, 4) as usize;
                let mut out = [0.0; 4];
                for (k, o) in out.iter_mut().enumerate() {
                    *o = if k < n { a[k] } else { b[k - n] };
                }
                out
            }
            UniformExpr::Min(a, b) => both(a.eval(inputs), b.eval(inputs), &f32::min),
            UniformExpr::Max(a, b) => both(a.eval(inputs), b.eval(inputs), &f32::max),
            UniformExpr::Fmod(a, b) => both(a.eval(inputs), b.eval(inputs), &|x, y| x % y),
            UniformExpr::Clamp { input, min, max } => {
                let (v, lo, hi) = (input.eval(inputs), min.eval(inputs), max.eval(inputs));
                [v[0].max(lo[0]).min(hi[0]), v[1].max(lo[1]).min(hi[1]), v[2].max(lo[2]).min(hi[2]), v[3].max(lo[3]).min(hi[3])]
            }
        }
    }

    /// The expression as JSON (`{"op": …}`), for a client that evaluates it each frame with [`UniformExpr::from_json`].
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        let one = |op: &str, x: &UniformExpr| json!({ "op": op, "x": x.to_json() });
        let two = |op: &str, a: &UniformExpr, b: &UniformExpr| json!({ "op": op, "a": a.to_json(), "b": b.to_json() });
        match self {
            UniformExpr::Constant { value, .. } => json!({ "op": "constant", "value": value }),
            UniformExpr::VectorParameter { name, default } => json!({ "op": "vector", "name": name, "default": default }),
            UniformExpr::ScalarParameter { name, default } => json!({ "op": "scalar", "name": name, "default": default }),
            UniformExpr::Texture { index } => json!({ "op": "texture", "index": index }),
            UniformExpr::TextureParameter { name, index } => json!({ "op": "textureParameter", "name": name, "index": index }),
            UniformExpr::FlipBookTextureParameter { index } => json!({ "op": "flipBook", "index": index }),
            UniformExpr::Time => json!({ "op": "time" }),
            UniformExpr::RealTime => json!({ "op": "realTime" }),
            UniformExpr::Sine { x, cosine } => json!({ "op": if *cosine { "cos" } else { "sin" }, "x": x.to_json() }),
            UniformExpr::SquareRoot(x) => one("sqrt", x),
            UniformExpr::Length(x) => one("length", x),
            UniformExpr::Periodic(x) => one("periodic", x),
            UniformExpr::Abs(x) => one("abs", x),
            UniformExpr::Floor(x) => one("floor", x),
            UniformExpr::Ceil(x) => one("ceil", x),
            UniformExpr::Frac(x) => one("frac", x),
            UniformExpr::FoldedMath { a, b, op } => two(["add", "sub", "mul", "div", "dot"].get(*op as usize).copied().unwrap_or("dot"), a, b),
            UniformExpr::AppendVector { a, b, components_a } => json!({ "op": "append", "a": a.to_json(), "b": b.to_json(), "components": components_a }),
            UniformExpr::Min(a, b) => two("min", a, b),
            UniformExpr::Max(a, b) => two("max", a, b),
            UniformExpr::Fmod(a, b) => two("fmod", a, b),
            UniformExpr::Clamp { input, min, max } => json!({ "op": "clamp", "x": input.to_json(), "min": min.to_json(), "max": max.to_json() }),
        }
    }

    /// Read [`UniformExpr::to_json`] back (`None` on anything else).
    pub fn from_json(j: &serde_json::Value) -> Option<UniformExpr> {
        let sub = |k: &str| UniformExpr::from_json(&j[k]).map(Box::new);
        let v4 = |k: &str| -> Option<[f32; 4]> {
            let a = j[k].as_array()?;
            Some([0, 1, 2, 3].map(|i| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32))
        };
        let name = || j["name"].as_str().map(str::to_string);
        let index = || j["index"].as_i64().unwrap_or(0) as i32;
        let fold = |op: u8| Some(UniformExpr::FoldedMath { a: sub("a")?, b: sub("b")?, op });
        Some(match j["op"].as_str()? {
            "constant" => UniformExpr::Constant { value: v4("value")?, value_type: 4 },
            "vector" => UniformExpr::VectorParameter { name: name()?, default: v4("default")? },
            "scalar" => UniformExpr::ScalarParameter { name: name()?, default: j["default"].as_f64()? as f32 },
            "texture" => UniformExpr::Texture { index: index() },
            "textureParameter" => UniformExpr::TextureParameter { name: name()?, index: index() },
            "flipBook" => UniformExpr::FlipBookTextureParameter { index: index() },
            "time" => UniformExpr::Time,
            "realTime" => UniformExpr::RealTime,
            "sin" => UniformExpr::Sine { x: sub("x")?, cosine: false },
            "cos" => UniformExpr::Sine { x: sub("x")?, cosine: true },
            "sqrt" => UniformExpr::SquareRoot(sub("x")?),
            "length" => UniformExpr::Length(sub("x")?),
            "periodic" => UniformExpr::Periodic(sub("x")?),
            "abs" => UniformExpr::Abs(sub("x")?),
            "floor" => UniformExpr::Floor(sub("x")?),
            "ceil" => UniformExpr::Ceil(sub("x")?),
            "frac" => UniformExpr::Frac(sub("x")?),
            "add" => fold(0)?,
            "sub" => fold(1)?,
            "mul" => fold(2)?,
            "div" => fold(3)?,
            "dot" => fold(4)?,
            "append" => UniformExpr::AppendVector { a: sub("a")?, b: sub("b")?, components_a: j["components"].as_i64().unwrap_or(0) as i32 },
            "min" => UniformExpr::Min(sub("a")?, sub("b")?),
            "max" => UniformExpr::Max(sub("a")?, sub("b")?),
            "fmod" => UniformExpr::Fmod(sub("a")?, sub("b")?),
            "clamp" => UniformExpr::Clamp { input: sub("x")?, min: sub("min")?, max: sub("max")? },
            _ => return None,
        })
    }

    /// Whether the value changes with the clock.
    pub fn uses_time(&self) -> bool {
        match self {
            UniformExpr::Time | UniformExpr::RealTime => true,
            UniformExpr::Sine { x, .. } | UniformExpr::SquareRoot(x) | UniformExpr::Length(x) | UniformExpr::Periodic(x) | UniformExpr::Abs(x) | UniformExpr::Floor(x) | UniformExpr::Ceil(x) | UniformExpr::Frac(x) => x.uses_time(),
            UniformExpr::FoldedMath { a, b, .. } | UniformExpr::AppendVector { a, b, .. } | UniformExpr::Min(a, b) | UniformExpr::Max(a, b) | UniformExpr::Fmod(a, b) => a.uses_time() || b.uses_time(),
            UniformExpr::Clamp { input, min, max } => input.uses_time() || min.uses_time() || max.uses_time(),
            _ => false,
        }
    }

    /// The texture a texture expression names: its parameter (a material instance may set it) and its index in the material's texture list.
    pub fn texture(&self) -> Option<(Option<&str>, i32)> {
        match self {
            UniformExpr::Texture { index } | UniformExpr::FlipBookTextureParameter { index } => Some((None, *index)),
            UniformExpr::TextureParameter { name, index } => Some((Some(name), *index)),
            _ => None,
        }
    }
}

/// `FShaderFrequencyUniformExpressions`: the expressions of one shader stage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrequencyExpressions {
    pub vectors: Vec<UniformExpr>,
    pub scalars: Vec<UniformExpr>,
    pub textures_2d: Vec<UniformExpr>,
}

impl FrequencyExpressions {
    fn read(r: &mut Reader, names: &dyn Fn(FName) -> String) -> Result<FrequencyExpressions> {
        let list = |r: &mut Reader| r.array(|r| UniformExpr::read(r, names));
        Ok(FrequencyExpressions { vectors: list(r)?, scalars: list(r)?, textures_2d: list(r)? })
    }
}

/// `FUniformExpressionSet`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UniformExpressionSet {
    pub pixel: FrequencyExpressions,
    pub cube_textures: Vec<UniformExpr>,
    pub vertex: FrequencyExpressions,
    pub hull: FrequencyExpressions,
    pub domain: FrequencyExpressions,
}

// ----- the cache of a package -----------------------------------------------------------

/// `TShaderMap`: shader type name → shader id.
pub type ShaderMap = Vec<(String, Guid)>;

/// `FMeshMaterialShaderMap`: the shaders drawing the material with one vertex factory.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshShaderMap {
    pub vertex_factory: String,
    pub shaders: ShaderMap,
}

/// `FMaterialShaderMap`: a material (and static parameter set) compiled for a platform.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialShaderMap {
    /// The key it is stored under (the material's id is its `base_material_id`).
    pub key: StaticParameterSet,
    pub shaders: ShaderMap,
    pub mesh_maps: Vec<MeshShaderMap>,
    pub material_id: Guid,
    pub friendly_name: String,
    pub static_parameters: StaticParameterSet,
    pub uniforms: UniformExpressionSet,
    pub platform: i32,
}

impl MaterialShaderMap {
    /// The shader of type `shader_type` for `vertex_factory` (names as the engine has them, case-insensitive).
    pub fn mesh_shader(&self, vertex_factory: &str, shader_type: &str) -> Option<Guid> {
        self.mesh_maps.iter().find(|m| m.vertex_factory.eq_ignore_ascii_case(vertex_factory))?.shaders.iter().find(|(t, _)| t.eq_ignore_ascii_case(shader_type)).map(|(_, id)| *id)
    }
}

/// One shader the cache lists.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedShader {
    pub type_name: String,
    pub id: Guid,
    /// `EShaderFrequency`: 0 vertex, 1 hull, 2 domain, 3 pixel, 4 geometry, 5 compute.
    pub frequency: u8,
    /// `EShaderPlatform` (0: PC Direct3D 9 shader model 3).
    pub platform: u8,
    pub parameter_map_crc: u32,
    pub num_instructions: u32,
    /// What the shader type serialises after `FShader::Serialize`: its
    /// parameters (`FShaderParameter` BaseIndex, NumBytes, BufferIndex;
    /// `FShaderResourceParameter` BaseIndex, NumResources, SamplerIndex; 3
    /// `u16`s each) in the type's own order, with whatever else it keeps.
    pub parameters: Vec<u8>,
}

/// A `ShaderCache` export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderCache {
    pub priority: i32,
    pub platform: u8,
    pub shaders: Vec<CachedShader>,
    pub material_maps: Vec<MaterialShaderMap>,
}

fn read_shader_map(r: &mut Reader, names: &dyn Fn(FName) -> String) -> Result<ShaderMap> {
    r.array(|r| {
        // `TMap<FShaderType*, TRefCountPtr<FShader>>`: the type, then the shader as its id and type.
        let ty = names(r.fname()?);
        let id = r.guid()?;
        let _ty_again = r.fname()?;
        Ok((ty, id))
    })
}

/// `-skip` when the stored offset is not positive: it is then relative to the position after it.
fn skip_target(r: &mut Reader) -> Result<usize> {
    let skip = r.i32()?;
    Ok(if skip <= 0 { (r.pos() as i64 - skip as i64) as usize } else { skip as usize })
}

impl ShaderCache {
    /// Read the native part of a `ShaderCache` export (what follows its properties); `names` resolves the package's names.
    ///
    /// The offsets stored in a cooked package are relative, so the payload reads from wherever it sits.
    pub fn read(r: &mut Reader, names: &dyn Fn(FName) -> String) -> Result<ShaderCache> {
        let priority = r.i32()?;
        let platform = r.u8()?;
        // `FCompressedShaderCodeCache`: a map by shader type, empty with the code storage.
        let compressed = r.i32()?;
        if compressed != 0 {
            return Err(Error::Format(format!("the shader cache holds {compressed} compressed code caches (not handled)")));
        }
        let count = r.i32()?.max(0) as usize;
        let mut shaders = Vec::with_capacity(count);
        for _ in 0..count {
            let type_name = names(r.fname()?);
            let id = r.guid()?;
            r.take(20)?; // FSHAHash, zeroed by the cooker
            let end = skip_target(r)?;
            // `FShader::Serialize`: what every shader stores; its type's parameters follow and are skipped.
            let _serializations = r.array(|r| r.u16())?;
            let platform = r.u8()?;
            let frequency = r.u8()?;
            let code = r.i32()?.max(0) as usize;
            r.take(code)?;
            let parameter_map_crc = r.u32()?;
            let _id = r.guid()?;
            let _type = r.fname()?;
            r.take(20)?;
            let num_instructions = r.u32()?;
            if end < r.pos() || end > r.data().len() {
                return Err(Error::Format(format!("shader {type_name} {id}: its skip offset {end} is out of range")));
            }
            let parameters = r.data()[r.pos()..end].to_vec();
            r.seek(end);
            shaders.push(CachedShader { type_name, id, frequency, platform, parameter_map_crc, num_instructions, parameters });
        }
        let count = r.i32()?.max(0) as usize;
        let mut material_maps = Vec::with_capacity(count);
        for _ in 0..count {
            let key = StaticParameterSet::read(r)?;
            let _version = r.i32()?;
            let _licensee_version = r.i32()?;
            let end = skip_target(r)?;
            let shaders = read_shader_map(r, names)?;
            let mesh_maps = r.array(|r| {
                let shaders = read_shader_map(r, names)?;
                Ok(MeshShaderMap { vertex_factory: names(r.fname()?), shaders })
            })?;
            let material_id = r.guid()?;
            let friendly_name = r.fstring()?;
            let static_parameters = StaticParameterSet::read(r)?;
            let uniforms = UniformExpressionSet {
                pixel: FrequencyExpressions::read(r, names)?,
                cube_textures: r.array(|r| UniformExpr::read(r, names))?,
                vertex: FrequencyExpressions::read(r, names)?,
                hull: FrequencyExpressions::read(r, names)?,
                domain: FrequencyExpressions::read(r, names)?,
            };
            let platform = r.i32()?;
            if r.pos() != end {
                return Err(Error::Format(format!("material shader map {friendly_name}: read to {} of {end}", r.pos())));
            }
            material_maps.push(MaterialShaderMap { key, shaders, mesh_maps, material_id, friendly_name, static_parameters, uniforms, platform });
        }
        Ok(ShaderCache { priority, platform, shaders, material_maps })
    }

    /// The shader maps of the material with resource id `id` (one per static parameter set of its instances).
    pub fn maps_of(&self, id: &Guid) -> impl Iterator<Item = &MaterialShaderMap> {
        let id = *id;
        self.material_maps.iter().filter(move |m| m.key.base_material_id == id)
    }
}

/// The shader caches of a package (`SeekFreeShaderCache` exports, one per platform), parsed.
pub fn package_caches(pkg: &tempest_upk::Package) -> Vec<Result<ShaderCache>> {
    let none = pkg.names.iter().position(|n| n.name.eq_ignore_ascii_case("None")).unwrap_or(usize::MAX) as u32;
    (0..pkg.exports.len())
        .filter(|&i| pkg.class_name_is(tempest_upk::io::PackageIndex::export(i), "ShaderCache"))
        .map(|i| {
            let data = pkg.export_data.get(i).map(|d| d.as_slice()).unwrap_or_default();
            let mut r = Reader::new(data);
            let _net_index = r.i32()?;
            // The class has no properties: the tagged list is its `None` end marker.
            let end = r.fname()?;
            if end.index != none {
                return Err(Error::Format("the shader cache has properties (not handled)".into()));
            }
            ShaderCache::read(&mut r, &|n| pkg.fname_to_string(n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct In;
    impl UniformInputs for In {
        fn scalar(&self, name: &str) -> Option<f32> {
            (name == "Glow").then_some(3.0)
        }
        fn vector(&self, _: &str) -> Option<[f32; 4]> {
            None
        }
        fn time(&self) -> (f32, f32) {
            (2.0, 5.0)
        }
    }

    #[test]
    fn uniform_expressions_evaluate_like_the_engine() {
        let c = |v: [f32; 4]| Box::new(UniformExpr::Constant { value: v, value_type: 4 });
        let glow = Box::new(UniformExpr::ScalarParameter { name: "Glow".into(), default: 1.0 });
        // A parameter set by the instance, one left at its default.
        assert_eq!(glow.eval(&In)[0], 3.0);
        assert_eq!(UniformExpr::ScalarParameter { name: "Other".into(), default: 1.5 }.eval(&In)[0], 1.5);
        // Glow * (0.5, 1, 2, 1) is per component over the scalar's red channel layout.
        let mul = UniformExpr::FoldedMath { a: c([3.0; 4]), b: c([0.5, 1.0, 2.0, 1.0]), op: 2 };
        assert_eq!(mul.eval(&In), [1.5, 3.0, 6.0, 3.0]);
        // Append: two components of a, then b's.
        let app = UniformExpr::AppendVector { a: c([1.0, 2.0, 9.0, 9.0]), b: c([7.0, 8.0, 0.0, 0.0]), components_a: 2 };
        assert_eq!(app.eval(&In), [1.0, 2.0, 7.0, 8.0]);
        assert_eq!(UniformExpr::Sine { x: Box::new(UniformExpr::Time), cosine: false }.eval(&In)[0], 2.0f32.sin());
        assert_eq!(UniformExpr::Periodic(c([2.25, -0.25, 0.0, 0.0])).eval(&In)[..2], [0.25, 0.75]);
        let clamp = UniformExpr::Clamp { input: c([5.0, -1.0, 0.5, 0.0]), min: c([0.0; 4]), max: c([1.0; 4]) };
        assert_eq!(clamp.eval(&In), [1.0, 0.0, 0.5, 0.0]);
    }

    #[test]
    fn uniform_expressions_survive_json() {
        let c = |v: [f32; 4]| Box::new(UniformExpr::Constant { value: v, value_type: 4 });
        let e = UniformExpr::Clamp {
            input: Box::new(UniformExpr::FoldedMath { a: Box::new(UniformExpr::Sine { x: Box::new(UniformExpr::Time), cosine: true }), b: Box::new(UniformExpr::ScalarParameter { name: "Glow".into(), default: 1.0 }), op: 2 }),
            min: c([0.0; 4]),
            max: Box::new(UniformExpr::AppendVector { a: c([1.0; 4]), b: Box::new(UniformExpr::VectorParameter { name: "Tint".into(), default: [0.5; 4] }), components_a: 1 }),
        };
        assert_eq!(UniformExpr::from_json(&e.to_json()), Some(e.clone()));
        assert!(e.uses_time());
        assert!(!UniformExpr::ScalarParameter { name: "Glow".into(), default: 1.0 }.uses_time());
    }

    #[test]
    fn skip_offsets_are_relative_when_not_positive() {
        // -8 stored at 4..8: the target is 8 bytes after the field.
        let data = [0u8, 0, 0, 0, 0xf8, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut r = Reader::new(&data);
        r.seek(4);
        assert_eq!(skip_target(&mut r).unwrap(), 16);
    }
}
