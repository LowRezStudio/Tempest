//! Animation sequences, light maps, primitive components with per-instance
//! lighting, BSP models and the level/world containers (UnSkeletalAnim.cpp,
//! UnLightMap.cpp, UnStaticMesh.cpp, UnBrushComponent.cpp,
//! UnModelComponent.cpp, UnModel.cpp, UnFPoly.cpp, UnLevel.cpp, UnWorld.cpp;
//! the 8.2 binary matches the source for all of them). Every version branch
//! is resolved for file version 893.

use tempest_upk::io::{Error, FName, Guid, PackageIndex, Reader, Result, Writer};

use crate::bulk::BulkData;
use crate::mesh::{BoxSphereBounds, BulkArray, ColorVertexBuffer, Layout, Vector};

fn read_vector(r: &mut Reader) -> Result<Vector> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}
fn write_vector(w: &mut Writer, v: &Vector) {
    for x in v {
        w.f32(*x);
    }
}

/// `FBox`: min, max, `IsValid` byte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Box3 {
    pub min: Vector,
    pub max: Vector,
    pub is_valid: u8,
}

impl Box3 {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Box3 { min: read_vector(r)?, max: read_vector(r)?, is_valid: r.u8()? })
    }
    fn write(&self, w: &mut Writer) {
        write_vector(w, &self.min);
        write_vector(w, &self.max);
        w.u8(self.is_valid);
    }
}

// ----------------------------------------------------------------------------
// Animation
// ----------------------------------------------------------------------------

/// `UAnimSequence::Serialize`: raw tracks (`PosKeys`, `RotKeys`, both
/// bulk-serialised) and the compressed byte stream (`NumBytes` + bytes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnimSequence {
    pub raw_tracks: Vec<(BulkArray, BulkArray)>,
    pub compressed: Vec<u8>,
}

impl AnimSequence {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let raw_tracks = r.array(|r| Ok((BulkArray::read(r)?, BulkArray::read(r)?)))?;
        let n = r.i32()?;
        if n < 0 {
            return Err(Error::Format("negative compressed size".into()));
        }
        Ok(AnimSequence { raw_tracks, compressed: r.take(n as usize)?.to_vec() })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.raw_tracks, |w, (p, q)| {
            p.write(w);
            q.write(w);
        });
        w.i32(self.compressed.len() as i32);
        w.bytes(&self.compressed);
    }
}

// ----------------------------------------------------------------------------
// Light maps
// ----------------------------------------------------------------------------

pub const NUM_STORED_LIGHTMAP_COEF: usize = 3;

/// `FLightMap*` (`operator<<(FArchive&, FLightMap*&)`): a type word, then
/// `FLightMap1D` or `FLightMap2D`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum LightMap {
    #[default]
    None,
    OneD {
        light_guids: Vec<Guid>,
        owner: PackageIndex,
        directional_samples: BulkData,
        scale_vectors: [Vector; NUM_STORED_LIGHTMAP_COEF],
        simple_samples: BulkData,
    },
    TwoD {
        light_guids: Vec<Guid>,
        textures: [(PackageIndex, Vector); NUM_STORED_LIGHTMAP_COEF],
        coordinate_scale: [f32; 2],
        coordinate_bias: [f32; 2],
    },
}

impl LightMap {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let ty = r.u32()?;
        Ok(match ty {
            0 => LightMap::None,
            1 => {
                let light_guids = r.array(|r| r.guid())?;
                let owner = r.index()?;
                let directional_samples = BulkData::read(r)?;
                let mut scale_vectors = [[0f32; 3]; NUM_STORED_LIGHTMAP_COEF];
                for v in scale_vectors.iter_mut() {
                    *v = read_vector(r)?;
                }
                let simple_samples = BulkData::read(r)?;
                LightMap::OneD { light_guids, owner, directional_samples, scale_vectors, simple_samples }
            }
            2 => {
                let light_guids = r.array(|r| r.guid())?;
                let mut textures = [(PackageIndex::NONE, [0f32; 3]); NUM_STORED_LIGHTMAP_COEF];
                for t in textures.iter_mut() {
                    *t = (r.index()?, read_vector(r)?);
                }
                let coordinate_scale = [r.f32()?, r.f32()?];
                let coordinate_bias = [r.f32()?, r.f32()?];
                LightMap::TwoD { light_guids, textures, coordinate_scale, coordinate_bias }
            }
            t => return Err(Error::Format(format!("unknown light map type {t}"))),
        })
    }

    pub fn write(&self, w: &mut Writer) {
        match self {
            LightMap::None => w.u32(0),
            LightMap::OneD { light_guids, owner, directional_samples, scale_vectors, simple_samples } => {
                w.u32(1);
                w.array(light_guids, |w, g| w.guid(g));
                w.index(*owner);
                directional_samples.write(w);
                for v in scale_vectors {
                    write_vector(w, v);
                }
                simple_samples.write(w);
            }
            LightMap::TwoD { light_guids, textures, coordinate_scale, coordinate_bias } => {
                w.u32(2);
                w.array(light_guids, |w, g| w.guid(g));
                for (t, v) in textures {
                    w.index(*t);
                    write_vector(w, v);
                }
                w.f32(coordinate_scale[0]);
                w.f32(coordinate_scale[1]);
                w.f32(coordinate_bias[0]);
                w.f32(coordinate_bias[1]);
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Components
// ----------------------------------------------------------------------------

/// `FColorVertexBuffer::Serialize` (stride, count, data when there are
/// vertices and the package is not server-cooked).
fn read_color_vb(r: &mut Reader, layout: Layout) -> Result<ColorVertexBuffer> {
    let mut vb = ColorVertexBuffer { stride: r.i32()?, num_vertices: r.i32()?, data: None };
    if !layout.server && vb.num_vertices > 0 {
        vb.data = Some(BulkArray::read(r)?);
    }
    Ok(vb)
}
fn write_color_vb(w: &mut Writer, vb: &ColorVertexBuffer) {
    w.i32(vb.stride);
    w.i32(vb.num_vertices);
    if let Some(d) = &vb.data {
        d.write(w);
    }
}

/// `FPaintedVertex`: position, packed normal, colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaintedVertex {
    pub position: Vector,
    pub normal: u32,
    pub color: [u8; 4],
}

/// `FStaticMeshComponentLODInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticMeshComponentLod {
    pub shadow_maps: Vec<PackageIndex>,
    pub shadow_vertex_buffers: Vec<PackageIndex>,
    pub light_map: LightMap,
    pub override_vertex_colors: Option<ColorVertexBuffer>,
    pub painted_vertices: Vec<PaintedVertex>,
}

impl StaticMeshComponentLod {
    fn read(r: &mut Reader, layout: Layout) -> Result<Self> {
        let shadow_maps = r.array(|r| r.index())?;
        let shadow_vertex_buffers = r.array(|r| r.index())?;
        let light_map = LightMap::read(r)?;
        let has_colors = r.u8()?;
        let override_vertex_colors = if has_colors != 0 { Some(read_color_vb(r, layout)?) } else { None };
        let painted_vertices = r.array(|r| Ok(PaintedVertex { position: read_vector(r)?, normal: r.u32()?, color: [r.u8()?, r.u8()?, r.u8()?, r.u8()?] }))?;
        Ok(StaticMeshComponentLod { shadow_maps, shadow_vertex_buffers, light_map, override_vertex_colors, painted_vertices })
    }
    fn write(&self, w: &mut Writer) {
        w.array(&self.shadow_maps, |w, i| w.index(*i));
        w.array(&self.shadow_vertex_buffers, |w, i| w.index(*i));
        self.light_map.write(w);
        w.u8(self.override_vertex_colors.is_some() as u8);
        if let Some(vb) = &self.override_vertex_colors {
            write_color_vb(w, vb);
        }
        w.array(&self.painted_vertices, |w, v| {
            write_vector(w, &v.position);
            w.u32(v.normal);
            w.bytes(&v.color);
        });
    }
}

/// `UStaticMeshComponent::Serialize`: `LODData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticMeshComponent {
    pub lod_data: Vec<StaticMeshComponentLod>,
}

impl StaticMeshComponent {
    pub fn read(r: &mut Reader, layout: Layout) -> Result<Self> {
        Ok(StaticMeshComponent { lod_data: r.array(|r| StaticMeshComponentLod::read(r, layout))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.lod_data, |w, l| l.write(w));
    }
}

/// `UBrushComponent::Serialize`: `FKCachedConvexData CachedPhysBrushData`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrushComponent {
    pub cached_phys_brush_data: Vec<BulkArray>,
}

impl BrushComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(BrushComponent { cached_phys_brush_data: r.array(BulkArray::read)? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.cached_phys_brush_data, |w, e| e.write(w));
    }
}

/// `FModelElement`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelElement {
    pub light_map: LightMap,
    pub component: PackageIndex,
    pub material: PackageIndex,
    pub nodes: Vec<u16>,
    pub shadow_maps: Vec<PackageIndex>,
    pub irrelevant_lights: Vec<Guid>,
}

/// `UModelComponent::Serialize`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelComponent {
    pub model: PackageIndex,
    pub zone_index: i32,
    pub elements: Vec<ModelElement>,
    pub component_index: u16,
    pub nodes: Vec<u16>,
}

impl ModelComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(ModelComponent {
            model: r.index()?,
            zone_index: r.i32()?,
            elements: r.array(|r| {
                Ok(ModelElement {
                    light_map: LightMap::read(r)?,
                    component: r.index()?,
                    material: r.index()?,
                    nodes: r.array(|r| r.u16())?,
                    shadow_maps: r.array(|r| r.index())?,
                    irrelevant_lights: r.array(|r| r.guid())?,
                })
            })?,
            component_index: r.u16()?,
            nodes: r.array(|r| r.u16())?,
        })
    }
    pub fn write(&self, w: &mut Writer) {
        w.index(self.model);
        w.i32(self.zone_index);
        w.array(&self.elements, |w, e| {
            e.light_map.write(w);
            w.index(e.component);
            w.index(e.material);
            w.array(&e.nodes, |w, n| w.u16(*n));
            w.array(&e.shadow_maps, |w, i| w.index(*i));
            w.array(&e.irrelevant_lights, |w, g| w.guid(g));
        });
        w.u16(self.component_index);
        w.array(&self.nodes, |w, n| w.u16(*n));
    }
}

// ----------------------------------------------------------------------------
// BSP
// ----------------------------------------------------------------------------

/// `FLightmassPrimitiveSettings` at version 893.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightmassPrimitiveSettings {
    pub use_two_sided_lighting: i32,
    pub shadow_indirect_only: i32,
    pub fully_occluded_samples_fraction: f32,
    pub use_emissive_for_static_lighting: i32,
    pub emissive_light_falloff_exponent: f32,
    pub emissive_light_explicit_influence_radius: f32,
    pub emissive_boost: f32,
    pub diffuse_boost: f32,
    pub specular_boost: f32,
}

impl LightmassPrimitiveSettings {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(LightmassPrimitiveSettings {
            use_two_sided_lighting: r.i32()?,
            shadow_indirect_only: r.i32()?,
            fully_occluded_samples_fraction: r.f32()?,
            use_emissive_for_static_lighting: r.i32()?,
            emissive_light_falloff_exponent: r.f32()?,
            emissive_light_explicit_influence_radius: r.f32()?,
            emissive_boost: r.f32()?,
            diffuse_boost: r.f32()?,
            specular_boost: r.f32()?,
        })
    }
    fn write(&self, w: &mut Writer) {
        w.i32(self.use_two_sided_lighting);
        w.i32(self.shadow_indirect_only);
        w.f32(self.fully_occluded_samples_fraction);
        w.i32(self.use_emissive_for_static_lighting);
        w.f32(self.emissive_light_falloff_exponent);
        w.f32(self.emissive_light_explicit_influence_radius);
        w.f32(self.emissive_boost);
        w.f32(self.diffuse_boost);
        w.f32(self.specular_boost);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BspSurf {
    pub material: PackageIndex,
    pub poly_flags: u32,
    pub p_base: i32,
    pub v_normal: i32,
    pub v_texture_u: i32,
    pub v_texture_v: i32,
    pub i_brush_poly: i32,
    pub actor: PackageIndex,
    pub plane: [f32; 4],
    pub shadow_map_scale: f32,
    pub lighting_channels: u32,
    pub i_lightmass_index: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneProperties {
    pub zone_actor: PackageIndex,
    pub connectivity: u64,
    pub visibility: u64,
    pub last_render_time: f32,
}

/// `UModel::Serialize`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub bounds: BoxSphereBounds,
    pub vectors: BulkArray,
    pub points: BulkArray,
    pub nodes: BulkArray,
    /// `TTransArray` owner (the model itself) written before `Surfs`.
    pub surfs_owner: PackageIndex,
    pub surfs: Vec<BspSurf>,
    pub verts: BulkArray,
    pub num_shared_sides: i32,
    /// `Zones[NumZones]`, written without a count of their own.
    pub zones: Vec<ZoneProperties>,
    pub polys: PackageIndex,
    pub leaf_hulls: BulkArray,
    pub leaves: BulkArray,
    pub root_outside: i32,
    pub linked: i32,
    pub portal_nodes: BulkArray,
    pub num_unique_vertices: i32,
    pub vertex_buffer: BulkArray,
    pub lighting_guid: Guid,
    pub lightmass_settings: Vec<LightmassPrimitiveSettings>,
}

impl Model {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let bounds = BoxSphereBounds::read(r)?;
        let vectors = BulkArray::read(r)?;
        let points = BulkArray::read(r)?;
        let nodes = BulkArray::read(r)?;
        let surfs_owner = r.index()?;
        let surfs = r.array(|r| {
            Ok(BspSurf {
                material: r.index()?,
                poly_flags: r.u32()?,
                p_base: r.i32()?,
                v_normal: r.i32()?,
                v_texture_u: r.i32()?,
                v_texture_v: r.i32()?,
                i_brush_poly: r.i32()?,
                actor: r.index()?,
                plane: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
                shadow_map_scale: r.f32()?,
                lighting_channels: r.u32()?,
                i_lightmass_index: r.i32()?,
            })
        })?;
        let verts = BulkArray::read(r)?;
        let num_shared_sides = r.i32()?;
        let num_zones = r.i32()?;
        if !(0..=64).contains(&num_zones) {
            return Err(Error::Format(format!("bad zone count {num_zones}")));
        }
        let mut zones = Vec::with_capacity(num_zones as usize);
        for _ in 0..num_zones {
            zones.push(ZoneProperties { zone_actor: r.index()?, connectivity: r.u64()?, visibility: r.u64()?, last_render_time: r.f32()? });
        }
        let polys = r.index()?;
        let leaf_hulls = BulkArray::read(r)?;
        let leaves = BulkArray::read(r)?;
        let root_outside = r.i32()?;
        let linked = r.i32()?;
        let portal_nodes = BulkArray::read(r)?;
        let num_unique_vertices = r.i32()?;
        let vertex_buffer = BulkArray::read(r)?;
        let lighting_guid = r.guid()?;
        let lightmass_settings = r.array(LightmassPrimitiveSettings::read)?;
        Ok(Model {
            bounds,
            vectors,
            points,
            nodes,
            surfs_owner,
            surfs,
            verts,
            num_shared_sides,
            zones,
            polys,
            leaf_hulls,
            leaves,
            root_outside,
            linked,
            portal_nodes,
            num_unique_vertices,
            vertex_buffer,
            lighting_guid,
            lightmass_settings,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        self.bounds.write(w);
        self.vectors.write(w);
        self.points.write(w);
        self.nodes.write(w);
        w.index(self.surfs_owner);
        w.array(&self.surfs, |w, s| {
            w.index(s.material);
            w.u32(s.poly_flags);
            w.i32(s.p_base);
            w.i32(s.v_normal);
            w.i32(s.v_texture_u);
            w.i32(s.v_texture_v);
            w.i32(s.i_brush_poly);
            w.index(s.actor);
            for x in s.plane {
                w.f32(x);
            }
            w.f32(s.shadow_map_scale);
            w.u32(s.lighting_channels);
            w.i32(s.i_lightmass_index);
        });
        self.verts.write(w);
        w.i32(self.num_shared_sides);
        w.i32(self.zones.len() as i32);
        for z in &self.zones {
            w.index(z.zone_actor);
            w.u64(z.connectivity);
            w.u64(z.visibility);
            w.f32(z.last_render_time);
        }
        w.index(self.polys);
        self.leaf_hulls.write(w);
        self.leaves.write(w);
        w.i32(self.root_outside);
        w.i32(self.linked);
        self.portal_nodes.write(w);
        w.i32(self.num_unique_vertices);
        self.vertex_buffer.write(w);
        w.guid(&self.lighting_guid);
        w.array(&self.lightmass_settings, |w, s| s.write(w));
    }
}

/// `FPoly` at version 893.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poly {
    pub base: Vector,
    pub normal: Vector,
    pub texture_u: Vector,
    pub texture_v: Vector,
    pub vertices: Vec<Vector>,
    pub poly_flags: u32,
    pub actor: PackageIndex,
    pub item_name: FName,
    pub material: PackageIndex,
    pub i_link: i32,
    pub i_brush_poly: i32,
    pub shadow_map_scale: f32,
    pub lighting_channels: u32,
    pub lightmass_settings: LightmassPrimitiveSettings,
    pub ruleset_variation: FName,
}

/// `UPolys::Serialize`: `DbNum`, `DbMax`, owner, then `DbNum` polys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polys {
    pub db_max: i32,
    pub owner: PackageIndex,
    pub elements: Vec<Poly>,
}

impl Polys {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let db_num = r.i32()?;
        let db_max = r.i32()?;
        let owner = r.index()?;
        if db_num < 0 {
            return Err(Error::Format("negative poly count".into()));
        }
        let mut elements = Vec::with_capacity(db_num.min(1 << 16) as usize);
        for _ in 0..db_num {
            elements.push(Poly {
                base: read_vector(r)?,
                normal: read_vector(r)?,
                texture_u: read_vector(r)?,
                texture_v: read_vector(r)?,
                vertices: r.array(read_vector)?,
                poly_flags: r.u32()?,
                actor: r.index()?,
                item_name: r.fname()?,
                material: r.index()?,
                i_link: r.i32()?,
                i_brush_poly: r.i32()?,
                shadow_map_scale: r.f32()?,
                lighting_channels: r.u32()?,
                lightmass_settings: LightmassPrimitiveSettings::read(r)?,
                ruleset_variation: r.fname()?,
            });
        }
        Ok(Polys { db_max, owner, elements })
    }
    pub fn write(&self, w: &mut Writer) {
        w.i32(self.elements.len() as i32);
        w.i32(self.db_max);
        w.index(self.owner);
        for p in &self.elements {
            write_vector(w, &p.base);
            write_vector(w, &p.normal);
            write_vector(w, &p.texture_u);
            write_vector(w, &p.texture_v);
            w.array(&p.vertices, write_vector);
            w.u32(p.poly_flags);
            w.index(p.actor);
            w.fname(p.item_name);
            w.index(p.material);
            w.i32(p.i_link);
            w.i32(p.i_brush_poly);
            w.f32(p.shadow_map_scale);
            w.u32(p.lighting_channels);
            p.lightmass_settings.write(w);
            w.fname(p.ruleset_variation);
        }
    }
}

// ----------------------------------------------------------------------------
// World and level
// ----------------------------------------------------------------------------

/// `FLevelViewportInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportInfo {
    pub cam_position: Vector,
    pub cam_rotation: [i32; 3],
    pub cam_ortho_zoom: f32,
}

/// `UWorld::Serialize`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    pub persistent_level: PackageIndex,
    pub persistent_facefx_anim_set: PackageIndex,
    pub editor_views: Vec<ViewportInfo>,
    pub save_game_summary: PackageIndex,
    pub extra_referenced_objects: Vec<PackageIndex>,
}

impl World {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let persistent_level = r.index()?;
        let persistent_facefx_anim_set = r.index()?;
        let mut editor_views = Vec::with_capacity(4);
        for _ in 0..4 {
            editor_views.push(ViewportInfo { cam_position: read_vector(r)?, cam_rotation: [r.i32()?, r.i32()?, r.i32()?], cam_ortho_zoom: r.f32()? });
        }
        let save_game_summary = r.index()?;
        let extra_referenced_objects = r.array(|r| r.index())?;
        Ok(World { persistent_level, persistent_facefx_anim_set, editor_views, save_game_summary, extra_referenced_objects })
    }
    pub fn write(&self, w: &mut Writer) {
        w.index(self.persistent_level);
        w.index(self.persistent_facefx_anim_set);
        for v in &self.editor_views {
            write_vector(w, &v.cam_position);
            for x in v.cam_rotation {
                w.i32(x);
            }
            w.f32(v.cam_ortho_zoom);
        }
        w.index(self.save_game_summary);
        w.array(&self.extra_referenced_objects, |w, i| w.index(*i));
    }
}

/// `FURL`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Url {
    pub protocol: String,
    pub host: String,
    pub map: String,
    pub portal: String,
    pub op: Vec<String>,
    pub port: i32,
    pub valid: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamableTextureInstance {
    pub bounding_sphere: [f32; 4],
    pub texel_factor: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicTextureInstance {
    pub bounding_sphere: [f32; 4],
    pub texel_factor: f32,
    pub texture: PackageIndex,
    pub attached: i32,
    pub original_radius: f32,
}

/// `FCachedPhysSMData` / `FCachedPerTriPhysSMData`: scale and store index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CachedPhysRef {
    pub scale_3d: Vector,
    pub cached_data_index: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeLightingSample {
    pub position: Vector,
    pub radius: f32,
    pub indirect_direction: [u8; 2],
    pub environment_direction: [u8; 2],
    pub indirect_radiance: [u8; 4],
    pub environment_radiance: [u8; 4],
    pub ambient_radiance: [u8; 4],
    pub shadowed_from_dominant_lights: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrecomputedLightVolume {
    pub initialized: i32,
    pub bounds: Box3,
    pub sample_spacing: f32,
    pub samples: Vec<VolumeLightingSample>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisibilityCell {
    pub min: Vector,
    pub chunk_index: u16,
    pub data_offset: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompressedVisibilityChunk {
    pub compressed: i32,
    pub uncompressed_size: i32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisibilityBucket {
    pub cell_data_size: i32,
    pub cells: Vec<VisibilityCell>,
    pub chunks: Vec<CompressedVisibilityChunk>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrecomputedVisibility {
    pub cell_bucket_origin_xy: [f32; 2],
    pub cell_size_xy: f32,
    pub cell_size_z: f32,
    pub cell_bucket_size_xy: i32,
    pub num_cell_buckets: i32,
    pub buckets: Vec<VisibilityBucket>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeDistanceField {
    pub max_distance: f32,
    pub volume_box: Box3,
    pub size: [i32; 3],
    pub data: Vec<[u8; 4]>,
}

/// `ULevel::Serialize` (with `ULevelBase::Serialize` first).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    /// `TTransArray` owner written before `Actors`.
    pub actors_owner: PackageIndex,
    pub actors: Vec<PackageIndex>,
    pub url: Url,
    pub model: PackageIndex,
    pub model_components: Vec<PackageIndex>,
    pub game_sequences: Vec<PackageIndex>,
    pub texture_to_instances: Vec<(PackageIndex, Vec<StreamableTextureInstance>)>,
    pub dynamic_texture_instances: Vec<(PackageIndex, Vec<DynamicTextureInstance>)>,
    /// APEX cached data (`DWORD` size + bytes).
    pub apex_data: Vec<u8>,
    pub cached_phys_bsp_data: BulkArray,
    pub cached_phys_sm_data_map: Vec<(PackageIndex, CachedPhysRef)>,
    pub cached_phys_sm_data_store: Vec<Vec<BulkArray>>,
    pub cached_phys_per_tri_sm_data_map: Vec<(PackageIndex, CachedPhysRef)>,
    pub cached_phys_per_tri_sm_data_store: Vec<BulkArray>,
    pub cached_phys_bsp_data_version: i32,
    pub cached_phys_sm_data_version: i32,
    pub force_stream_textures: Vec<(PackageIndex, i32)>,
    pub cached_phys_convex_bsp_data: Vec<BulkArray>,
    pub cached_phys_convex_bsp_version: i32,
    pub nav_list: (PackageIndex, PackageIndex),
    pub cover_list: (PackageIndex, PackageIndex),
    pub pylon_list: (PackageIndex, PackageIndex),
    pub cross_level_cover_guid_refs: Vec<(Guid, u32)>,
    pub cover_link_refs: Vec<PackageIndex>,
    pub cover_index_pairs: Vec<(u32, u8)>,
    pub cross_level_actors: Vec<PackageIndex>,
    pub precomputed_light_volume: PrecomputedLightVolume,
    pub precomputed_visibility: PrecomputedVisibility,
    pub volume_distance_field: VolumeDistanceField,
}

fn read_sphere(r: &mut Reader) -> Result<[f32; 4]> {
    Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?])
}

impl Level {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let actors_owner = r.index()?;
        let actors = r.array(|r| r.index())?;
        let url = Url { protocol: r.fstring()?, host: r.fstring()?, map: r.fstring()?, portal: r.fstring()?, op: r.array(|r| r.fstring())?, port: r.i32()?, valid: r.i32()? };
        let model = r.index()?;
        let model_components = r.array(|r| r.index())?;
        let game_sequences = r.array(|r| r.index())?;
        let texture_to_instances = r.array(|r| Ok((r.index()?, r.array(|r| Ok(StreamableTextureInstance { bounding_sphere: read_sphere(r)?, texel_factor: r.f32()? }))?)))?;
        let dynamic_texture_instances = r.array(|r| {
            Ok((
                r.index()?,
                r.array(|r| Ok(DynamicTextureInstance { bounding_sphere: read_sphere(r)?, texel_factor: r.f32()?, texture: r.index()?, attached: r.i32()?, original_radius: r.f32()? }))?,
            ))
        })?;
        let apex_len = r.u32()? as usize;
        let apex_data = r.take(apex_len)?.to_vec();
        let cached_phys_bsp_data = BulkArray::read(r)?;
        let cached_phys_sm_data_map = r.array(|r| Ok((r.index()?, CachedPhysRef { scale_3d: read_vector(r)?, cached_data_index: r.i32()? })))?;
        let cached_phys_sm_data_store = r.array(|r| r.array(BulkArray::read))?;
        let cached_phys_per_tri_sm_data_map = r.array(|r| Ok((r.index()?, CachedPhysRef { scale_3d: read_vector(r)?, cached_data_index: r.i32()? })))?;
        let cached_phys_per_tri_sm_data_store = r.array(BulkArray::read)?;
        let cached_phys_bsp_data_version = r.i32()?;
        let cached_phys_sm_data_version = r.i32()?;
        let force_stream_textures = r.array(|r| Ok((r.index()?, r.i32()?)))?;
        let cached_phys_convex_bsp_data = r.array(BulkArray::read)?;
        let cached_phys_convex_bsp_version = r.i32()?;
        let nav_list = (r.index()?, r.index()?);
        let cover_list = (r.index()?, r.index()?);
        let pylon_list = (r.index()?, r.index()?);
        let cross_level_cover_guid_refs = r.array(|r| Ok((r.guid()?, r.u32()?)))?;
        let cover_link_refs = r.array(|r| r.index())?;
        let cover_index_pairs = r.array(|r| Ok((r.u32()?, r.u8()?)))?;
        let cross_level_actors = r.array(|r| r.index())?;
        let initialized = r.i32()?;
        let precomputed_light_volume = if initialized != 0 {
            let bounds = Box3::read(r)?;
            let sample_spacing = r.f32()?;
            let samples = r.array(|r| {
                Ok(VolumeLightingSample {
                    position: read_vector(r)?,
                    radius: r.f32()?,
                    indirect_direction: [r.u8()?, r.u8()?],
                    environment_direction: [r.u8()?, r.u8()?],
                    indirect_radiance: [r.u8()?, r.u8()?, r.u8()?, r.u8()?],
                    environment_radiance: [r.u8()?, r.u8()?, r.u8()?, r.u8()?],
                    ambient_radiance: [r.u8()?, r.u8()?, r.u8()?, r.u8()?],
                    shadowed_from_dominant_lights: r.u8()?,
                })
            })?;
            PrecomputedLightVolume { initialized, bounds, sample_spacing, samples }
        } else {
            PrecomputedLightVolume { initialized, ..Default::default() }
        };
        let precomputed_visibility = PrecomputedVisibility {
            cell_bucket_origin_xy: [r.f32()?, r.f32()?],
            cell_size_xy: r.f32()?,
            cell_size_z: r.f32()?,
            cell_bucket_size_xy: r.i32()?,
            num_cell_buckets: r.i32()?,
            buckets: r.array(|r| {
                Ok(VisibilityBucket {
                    cell_data_size: r.i32()?,
                    cells: r.array(|r| Ok(VisibilityCell { min: read_vector(r)?, chunk_index: r.u16()?, data_offset: r.u16()? }))?,
                    chunks: r.array(|r| Ok(CompressedVisibilityChunk { compressed: r.i32()?, uncompressed_size: r.i32()?, data: r.array(|r| r.u8())? }))?,
                })
            })?,
        };
        let volume_distance_field = VolumeDistanceField {
            max_distance: r.f32()?,
            volume_box: Box3::read(r)?,
            size: [r.i32()?, r.i32()?, r.i32()?],
            data: r.array(|r| Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?]))?,
        };
        Ok(Level {
            actors_owner,
            actors,
            url,
            model,
            model_components,
            game_sequences,
            texture_to_instances,
            dynamic_texture_instances,
            apex_data,
            cached_phys_bsp_data,
            cached_phys_sm_data_map,
            cached_phys_sm_data_store,
            cached_phys_per_tri_sm_data_map,
            cached_phys_per_tri_sm_data_store,
            cached_phys_bsp_data_version,
            cached_phys_sm_data_version,
            force_stream_textures,
            cached_phys_convex_bsp_data,
            cached_phys_convex_bsp_version,
            nav_list,
            cover_list,
            pylon_list,
            cross_level_cover_guid_refs,
            cover_link_refs,
            cover_index_pairs,
            cross_level_actors,
            precomputed_light_volume,
            precomputed_visibility,
            volume_distance_field,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.index(self.actors_owner);
        w.array(&self.actors, |w, i| w.index(*i));
        w.fstring(&self.url.protocol);
        w.fstring(&self.url.host);
        w.fstring(&self.url.map);
        w.fstring(&self.url.portal);
        w.array(&self.url.op, |w, s| w.fstring(s));
        w.i32(self.url.port);
        w.i32(self.url.valid);
        w.index(self.model);
        w.array(&self.model_components, |w, i| w.index(*i));
        w.array(&self.game_sequences, |w, i| w.index(*i));
        w.array(&self.texture_to_instances, |w, (t, v)| {
            w.index(*t);
            w.array(v, |w, s| {
                for x in s.bounding_sphere {
                    w.f32(x);
                }
                w.f32(s.texel_factor);
            });
        });
        w.array(&self.dynamic_texture_instances, |w, (t, v)| {
            w.index(*t);
            w.array(v, |w, s| {
                for x in s.bounding_sphere {
                    w.f32(x);
                }
                w.f32(s.texel_factor);
                w.index(s.texture);
                w.i32(s.attached);
                w.f32(s.original_radius);
            });
        });
        w.u32(self.apex_data.len() as u32);
        w.bytes(&self.apex_data);
        self.cached_phys_bsp_data.write(w);
        let write_ref = |w: &mut Writer, (i, c): &(PackageIndex, CachedPhysRef)| {
            w.index(*i);
            write_vector(w, &c.scale_3d);
            w.i32(c.cached_data_index);
        };
        w.array(&self.cached_phys_sm_data_map, write_ref);
        w.array(&self.cached_phys_sm_data_store, |w, d| w.array(d, |w, e| e.write(w)));
        w.array(&self.cached_phys_per_tri_sm_data_map, write_ref);
        w.array(&self.cached_phys_per_tri_sm_data_store, |w, e| e.write(w));
        w.i32(self.cached_phys_bsp_data_version);
        w.i32(self.cached_phys_sm_data_version);
        w.array(&self.force_stream_textures, |w, (t, b)| {
            w.index(*t);
            w.i32(*b);
        });
        w.array(&self.cached_phys_convex_bsp_data, |w, e| e.write(w));
        w.i32(self.cached_phys_convex_bsp_version);
        for p in [self.nav_list, self.cover_list, self.pylon_list] {
            w.index(p.0);
            w.index(p.1);
        }
        w.array(&self.cross_level_cover_guid_refs, |w, (g, i)| {
            w.guid(g);
            w.u32(*i);
        });
        w.array(&self.cover_link_refs, |w, i| w.index(*i));
        w.array(&self.cover_index_pairs, |w, (a, b)| {
            w.u32(*a);
            w.u8(*b);
        });
        w.array(&self.cross_level_actors, |w, i| w.index(*i));
        let v = &self.precomputed_light_volume;
        w.i32(v.initialized);
        if v.initialized != 0 {
            v.bounds.write(w);
            w.f32(v.sample_spacing);
            w.array(&v.samples, |w, s| {
                write_vector(w, &s.position);
                w.f32(s.radius);
                w.bytes(&s.indirect_direction);
                w.bytes(&s.environment_direction);
                w.bytes(&s.indirect_radiance);
                w.bytes(&s.environment_radiance);
                w.bytes(&s.ambient_radiance);
                w.u8(s.shadowed_from_dominant_lights);
            });
        }
        let p = &self.precomputed_visibility;
        w.f32(p.cell_bucket_origin_xy[0]);
        w.f32(p.cell_bucket_origin_xy[1]);
        w.f32(p.cell_size_xy);
        w.f32(p.cell_size_z);
        w.i32(p.cell_bucket_size_xy);
        w.i32(p.num_cell_buckets);
        w.array(&p.buckets, |w, b| {
            w.i32(b.cell_data_size);
            w.array(&b.cells, |w, c| {
                write_vector(w, &c.min);
                w.u16(c.chunk_index);
                w.u16(c.data_offset);
            });
            w.array(&b.chunks, |w, c| {
                w.i32(c.compressed);
                w.i32(c.uncompressed_size);
                w.array(&c.data, |w, b| w.u8(*b));
            });
        });
        let d = &self.volume_distance_field;
        w.f32(d.max_distance);
        d.volume_box.write(w);
        for s in d.size {
            w.i32(s);
        }
        w.array(&d.data, |w, c| w.bytes(c));
    }
}
