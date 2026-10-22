//! Static and skeletal mesh payloads, body setups and physics asset
//! instances (UnStaticMesh.cpp, UnSkeletalMesh.cpp, RawIndexBuffer.cpp,
//! UnPhysAsset.cpp; the 8.2 server binary matches the 10897 source for all
//! of them).
//!
//! GPU-only buffers (`FStaticMeshVertexBuffer`, `FColorVertexBuffer`, the
//! skeletal GPU skin and colour buffers) are not written when cooking for
//! `PLATFORM_WindowsServer`; [`Layout::server`] selects that form.

use tempest_upk::io::{Error, FName, Guid, PackageIndex, Reader, Result, Writer};

use crate::bulk::BulkData;

/// Which cooking target produced the package.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub server: bool,
}

// ----------------------------------------------------------------------------
// Primitives
// ----------------------------------------------------------------------------

pub type Vector = [f32; 3];
pub type Quat = [f32; 4];
pub type Rotator = [i32; 3];

fn read_vector(r: &mut Reader) -> Result<Vector> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}
fn write_vector(w: &mut Writer, v: &Vector) {
    for x in v {
        w.f32(*x);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoxSphereBounds {
    pub origin: Vector,
    pub box_extent: Vector,
    pub sphere_radius: f32,
}

impl BoxSphereBounds {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(BoxSphereBounds { origin: read_vector(r)?, box_extent: read_vector(r)?, sphere_radius: r.f32()? })
    }
    pub fn write(&self, w: &mut Writer) {
        write_vector(w, &self.origin);
        write_vector(w, &self.box_extent);
        w.f32(self.sphere_radius);
    }
}

/// `TArray<T>::BulkSerialize`: element size, count, then the raw elements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkArray {
    pub elem_size: i32,
    pub count: i32,
    pub data: Vec<u8>,
}

impl BulkArray {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let elem_size = r.i32()?;
        let count = r.i32()?;
        if elem_size < 0 || count < 0 {
            return Err(Error::Format(format!("bad bulk array ({elem_size} x {count})")));
        }
        let n = (elem_size as usize).checked_mul(count as usize).ok_or_else(|| Error::Format("bulk array overflow".into()))?;
        Ok(BulkArray { elem_size, count, data: r.take(n)?.to_vec() })
    }
    pub fn write(&self, w: &mut Writer) {
        w.i32(self.elem_size);
        w.i32(self.count);
        w.bytes(&self.data);
    }
    pub fn from_elems(elem_size: usize, data: Vec<u8>) -> BulkArray {
        BulkArray { elem_size: elem_size as i32, count: (data.len() / elem_size.max(1)) as i32, data }
    }
    pub fn elem(&self, i: usize) -> &[u8] {
        let s = self.elem_size as usize;
        &self.data[i * s..(i + 1) * s]
    }
}

/// `TkDOPTreeCompact`: root bound (3 min + 3 max planes), nodes, triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KDopTree {
    pub root_min: Vector,
    pub root_max: Vector,
    pub nodes: BulkArray,
    pub triangles: BulkArray,
}

impl KDopTree {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(KDopTree { root_min: read_vector(r)?, root_max: read_vector(r)?, nodes: BulkArray::read(r)?, triangles: BulkArray::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        write_vector(w, &self.root_min);
        write_vector(w, &self.root_max);
        self.nodes.write(w);
        self.triangles.write(w);
    }
}

// ----------------------------------------------------------------------------
// Static mesh
// ----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticMeshElement {
    pub material: PackageIndex,
    pub enable_collision: i32,
    pub old_enable_collision: i32,
    pub enable_shadow_casting: i32,
    pub first_index: u32,
    pub num_triangles: u32,
    pub min_vertex_index: u32,
    pub max_vertex_index: u32,
    pub material_index: i32,
    /// `FFragmentRange` (`BaseIndex`, `NumPrimitives`).
    pub fragments: Vec<(i32, i32)>,
    /// `LoadPlatformData` flag (PS3 data follows when set; never on PC).
    pub platform_data: u8,
}

impl StaticMeshElement {
    fn read(r: &mut Reader) -> Result<Self> {
        let e = StaticMeshElement {
            material: r.index()?,
            enable_collision: r.i32()?,
            old_enable_collision: r.i32()?,
            enable_shadow_casting: r.i32()?,
            first_index: r.u32()?,
            num_triangles: r.u32()?,
            min_vertex_index: r.u32()?,
            max_vertex_index: r.u32()?,
            material_index: r.i32()?,
            fragments: r.array(|r| Ok((r.i32()?, r.i32()?)))?,
            platform_data: r.u8()?,
        };
        if e.platform_data != 0 {
            return Err(Error::Format("static mesh element carries platform (PS3) data".into()));
        }
        Ok(e)
    }
    fn write(&self, w: &mut Writer) {
        w.index(self.material);
        w.i32(self.enable_collision);
        w.i32(self.old_enable_collision);
        w.i32(self.enable_shadow_casting);
        w.u32(self.first_index);
        w.u32(self.num_triangles);
        w.u32(self.min_vertex_index);
        w.u32(self.max_vertex_index);
        w.i32(self.material_index);
        w.array(&self.fragments, |w, (a, b)| {
            w.i32(*a);
            w.i32(*b);
        });
        w.u8(self.platform_data);
    }
}

/// `FPositionVertexBuffer`: stride, count, bulk data (`FPositionVertex` = 12 bytes).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PositionVertexBuffer {
    pub stride: i32,
    pub num_vertices: i32,
    pub data: BulkArray,
}

/// `FStaticMeshVertexBuffer`: tangents + UVs; data absent in server packages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticMeshVertexBuffer {
    pub num_tex_coords: u32,
    pub stride: u32,
    pub num_vertices: u32,
    pub use_full_precision_uvs: i32,
    pub data: Option<BulkArray>,
}

/// `FColorVertexBuffer`: data only when there are vertices (and not server).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColorVertexBuffer {
    pub stride: i32,
    pub num_vertices: i32,
    pub data: Option<BulkArray>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticMeshRenderData {
    pub raw_triangles: BulkData,
    pub elements: Vec<StaticMeshElement>,
    pub position_vb: PositionVertexBuffer,
    pub vertex_buffer: StaticMeshVertexBuffer,
    pub color_vb: ColorVertexBuffer,
    pub num_vertices: u32,
    /// `FRawStaticIndexBuffer` (16-bit indices).
    pub index_buffer: BulkArray,
    pub wireframe_index_buffer: BulkArray,
    pub adjacency_index_buffer: BulkArray,
}

impl StaticMeshRenderData {
    pub fn read(r: &mut Reader, layout: Layout) -> Result<Self> {
        let raw_triangles = BulkData::read(r)?;
        let elements = r.array(StaticMeshElement::read)?;
        let position_vb = PositionVertexBuffer { stride: r.i32()?, num_vertices: r.i32()?, data: BulkArray::read(r)? };
        let mut vertex_buffer = StaticMeshVertexBuffer { num_tex_coords: r.u32()?, stride: r.u32()?, num_vertices: r.u32()?, use_full_precision_uvs: r.i32()?, data: None };
        if !layout.server {
            vertex_buffer.data = Some(BulkArray::read(r)?);
        }
        let mut color_vb = ColorVertexBuffer { stride: r.i32()?, num_vertices: r.i32()?, data: None };
        if !layout.server && color_vb.num_vertices > 0 {
            color_vb.data = Some(BulkArray::read(r)?);
        }
        let num_vertices = r.u32()?;
        let index_buffer = BulkArray::read(r)?;
        let wireframe_index_buffer = BulkArray::read(r)?;
        let adjacency_index_buffer = BulkArray::read(r)?;
        Ok(StaticMeshRenderData { raw_triangles, elements, position_vb, vertex_buffer, color_vb, num_vertices, index_buffer, wireframe_index_buffer, adjacency_index_buffer })
    }

    pub fn write(&self, w: &mut Writer) {
        self.raw_triangles.write(w);
        w.array(&self.elements, |w, e| e.write(w));
        w.i32(self.position_vb.stride);
        w.i32(self.position_vb.num_vertices);
        self.position_vb.data.write(w);
        w.u32(self.vertex_buffer.num_tex_coords);
        w.u32(self.vertex_buffer.stride);
        w.u32(self.vertex_buffer.num_vertices);
        w.i32(self.vertex_buffer.use_full_precision_uvs);
        if let Some(d) = &self.vertex_buffer.data {
            d.write(w);
        }
        w.i32(self.color_vb.stride);
        w.i32(self.color_vb.num_vertices);
        if let Some(d) = &self.color_vb.data {
            d.write(w);
        }
        w.u32(self.num_vertices);
        self.index_buffer.write(w);
        self.wireframe_index_buffer.write(w);
        self.adjacency_index_buffer.write(w);
    }
}

/// `FStaticMeshOptimizationSettings` (version ≥ 863 layout).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptimizationSettings {
    pub reduction_method: u8,
    pub max_deviation_percentage: f32,
    pub num_of_triangles_percentage: f32,
    pub silhouette_importance: u8,
    pub texture_importance: u8,
    pub shading_importance: u8,
    pub recalc_normals: i32,
    pub normals_threshold: f32,
    pub welding_threshold: f32,
}

impl OptimizationSettings {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(OptimizationSettings {
            reduction_method: r.u8()?,
            max_deviation_percentage: r.f32()?,
            num_of_triangles_percentage: r.f32()?,
            silhouette_importance: r.u8()?,
            texture_importance: r.u8()?,
            shading_importance: r.u8()?,
            recalc_normals: r.i32()?,
            normals_threshold: r.f32()?,
            welding_threshold: r.f32()?,
        })
    }
    fn write(&self, w: &mut Writer) {
        w.u8(self.reduction_method);
        w.f32(self.max_deviation_percentage);
        w.f32(self.num_of_triangles_percentage);
        w.u8(self.silhouette_importance);
        w.u8(self.texture_importance);
        w.u8(self.shading_importance);
        w.i32(self.recalc_normals);
        w.f32(self.normals_threshold);
        w.f32(self.welding_threshold);
    }
}

/// `UStaticMesh::Serialize` at file version 893.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticMesh {
    pub bounds: BoxSphereBounds,
    pub body_setup: PackageIndex,
    pub kdop_tree: KDopTree,
    pub internal_version: i32,
    /// `FStaticMeshSourceData`: editor copy of the render data.
    pub source_data: Option<Box<StaticMeshRenderData>>,
    pub optimization_settings: Vec<OptimizationSettings>,
    pub has_been_simplified: i32,
    pub is_mesh_proxy: i32,
    pub lod_models: Vec<StaticMeshRenderData>,
    /// `LODInfo` elements serialise nothing: only their count survives.
    pub lod_info_count: i32,
    pub thumbnail_angle: Rotator,
    pub thumbnail_distance: f32,
    pub high_res_source_mesh_name: String,
    pub high_res_source_mesh_crc: u32,
    pub lighting_guid: Guid,
    pub vertex_position_version_number: i32,
    pub cached_streaming_texture_factors: Vec<f32>,
    pub remove_degenerates: i32,
    pub per_lod_static_lighting_for_instancing: i32,
    pub console_preallocate_instance_count: i32,
}

impl StaticMesh {
    pub fn read(r: &mut Reader, layout: Layout) -> Result<Self> {
        let bounds = BoxSphereBounds::read(r)?;
        let body_setup = r.index()?;
        let kdop_tree = KDopTree::read(r)?;
        let internal_version = r.i32()?;
        let has_source = r.i32()?;
        let source_data = if has_source != 0 { Some(Box::new(StaticMeshRenderData::read(r, layout)?)) } else { None };
        let optimization_settings = r.array(OptimizationSettings::read)?;
        let has_been_simplified = r.i32()?;
        let is_mesh_proxy = r.i32()?;
        let lod_models = r.array(|r| StaticMeshRenderData::read(r, layout))?;
        let lod_info_count = r.i32()?;
        let thumbnail_angle = [r.i32()?, r.i32()?, r.i32()?];
        let thumbnail_distance = r.f32()?;
        let high_res_source_mesh_name = r.fstring()?;
        let high_res_source_mesh_crc = r.u32()?;
        let lighting_guid = r.guid()?;
        let vertex_position_version_number = r.i32()?;
        let cached_streaming_texture_factors = r.array(|r| r.f32())?;
        let remove_degenerates = r.i32()?;
        let per_lod_static_lighting_for_instancing = r.i32()?;
        let console_preallocate_instance_count = r.i32()?;
        Ok(StaticMesh {
            bounds,
            body_setup,
            kdop_tree,
            internal_version,
            source_data,
            optimization_settings,
            has_been_simplified,
            is_mesh_proxy,
            lod_models,
            lod_info_count,
            thumbnail_angle,
            thumbnail_distance,
            high_res_source_mesh_name,
            high_res_source_mesh_crc,
            lighting_guid,
            vertex_position_version_number,
            cached_streaming_texture_factors,
            remove_degenerates,
            per_lod_static_lighting_for_instancing,
            console_preallocate_instance_count,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        self.bounds.write(w);
        w.index(self.body_setup);
        self.kdop_tree.write(w);
        w.i32(self.internal_version);
        w.i32(self.source_data.is_some() as i32);
        if let Some(s) = &self.source_data {
            s.write(w);
        }
        w.array(&self.optimization_settings, |w, s| s.write(w));
        w.i32(self.has_been_simplified);
        w.i32(self.is_mesh_proxy);
        w.array(&self.lod_models, |w, m| m.write(w));
        w.i32(self.lod_info_count);
        for a in self.thumbnail_angle {
            w.i32(a);
        }
        w.f32(self.thumbnail_distance);
        w.fstring(&self.high_res_source_mesh_name);
        w.u32(self.high_res_source_mesh_crc);
        w.guid(&self.lighting_guid);
        w.i32(self.vertex_position_version_number);
        w.array(&self.cached_streaming_texture_factors, |w, f| w.f32(*f));
        w.i32(self.remove_degenerates);
        w.i32(self.per_lod_static_lighting_for_instancing);
        w.i32(self.console_preallocate_instance_count);
    }
}

// ----------------------------------------------------------------------------
// Skeletal mesh
// ----------------------------------------------------------------------------

pub const MAX_TEXCOORDS: usize = 4;
pub const MAX_INFLUENCES: usize = 4;

/// `FMeshBone` with `VJointPos` (orientation + position only on disk).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshBone {
    pub name: FName,
    pub flags: u32,
    pub orientation: Quat,
    pub position: Vector,
    pub num_children: i32,
    pub parent_index: i32,
    pub color: [u8; 4],
}

impl MeshBone {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(MeshBone {
            name: r.fname()?,
            flags: r.u32()?,
            orientation: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
            position: read_vector(r)?,
            num_children: r.i32()?,
            parent_index: r.i32()?,
            color: [r.u8()?, r.u8()?, r.u8()?, r.u8()?],
        })
    }
    fn write(&self, w: &mut Writer) {
        w.fname(self.name);
        w.u32(self.flags);
        for x in self.orientation {
            w.f32(x);
        }
        write_vector(w, &self.position);
        w.i32(self.num_children);
        w.i32(self.parent_index);
        for c in self.color {
            w.u8(c);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkelMeshSection {
    pub material_index: u16,
    pub chunk_index: u16,
    pub base_index: u32,
    pub num_triangles: u32,
    pub triangle_sorting: u8,
}

impl SkelMeshSection {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(SkelMeshSection { material_index: r.u16()?, chunk_index: r.u16()?, base_index: r.u32()?, num_triangles: r.u32()?, triangle_sorting: r.u8()? })
    }
    fn write(&self, w: &mut Writer) {
        w.u16(self.material_index);
        w.u16(self.chunk_index);
        w.u32(self.base_index);
        w.u32(self.num_triangles);
        w.u8(self.triangle_sorting);
    }
}

/// `FMultiSizeIndexContainer`: CPU access flag, index size (2 or 4), bulk indices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiSizeIndexContainer {
    pub needs_cpu_access: i32,
    pub data_type_size: u8,
    pub indices: BulkArray,
}

impl MultiSizeIndexContainer {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(MultiSizeIndexContainer { needs_cpu_access: r.i32()?, data_type_size: r.u8()?, indices: BulkArray::read(r)? })
    }
    fn write(&self, w: &mut Writer) {
        w.i32(self.needs_cpu_access);
        w.u8(self.data_type_size);
        self.indices.write(w);
    }
    /// Indices widened to u32.
    pub fn to_u32(&self) -> Vec<u32> {
        let d = &self.indices.data;
        match self.indices.elem_size {
            2 => d.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect(),
            4 => d.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
            _ => Vec::new(),
        }
    }
}

/// `FRigidSkinVertex` (61 bytes on disk).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigidSkinVertex {
    pub position: Vector,
    pub tangent_x: u32,
    pub tangent_y: u32,
    pub tangent_z: u32,
    pub uvs: [[f32; 2]; MAX_TEXCOORDS],
    pub color: [u8; 4],
    pub bone: u8,
}

/// `FSoftSkinVertex` (68 bytes on disk).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoftSkinVertex {
    pub position: Vector,
    pub tangent_x: u32,
    pub tangent_y: u32,
    pub tangent_z: u32,
    pub uvs: [[f32; 2]; MAX_TEXCOORDS],
    pub color: [u8; 4],
    pub influence_bones: [u8; MAX_INFLUENCES],
    pub influence_weights: [u8; MAX_INFLUENCES],
}

fn read_uvs(r: &mut Reader) -> Result<[[f32; 2]; MAX_TEXCOORDS]> {
    let mut uvs = [[0f32; 2]; MAX_TEXCOORDS];
    for uv in uvs.iter_mut() {
        *uv = [r.f32()?, r.f32()?];
    }
    Ok(uvs)
}
fn write_uvs(w: &mut Writer, uvs: &[[f32; 2]; MAX_TEXCOORDS]) {
    for uv in uvs {
        w.f32(uv[0]);
        w.f32(uv[1]);
    }
}

impl RigidSkinVertex {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(RigidSkinVertex { position: read_vector(r)?, tangent_x: r.u32()?, tangent_y: r.u32()?, tangent_z: r.u32()?, uvs: read_uvs(r)?, color: [r.u8()?, r.u8()?, r.u8()?, r.u8()?], bone: r.u8()? })
    }
    fn write(&self, w: &mut Writer) {
        write_vector(w, &self.position);
        w.u32(self.tangent_x);
        w.u32(self.tangent_y);
        w.u32(self.tangent_z);
        write_uvs(w, &self.uvs);
        w.bytes(&self.color);
        w.u8(self.bone);
    }
}

impl SoftSkinVertex {
    fn read(r: &mut Reader) -> Result<Self> {
        let position = read_vector(r)?;
        let (tangent_x, tangent_y, tangent_z) = (r.u32()?, r.u32()?, r.u32()?);
        let uvs = read_uvs(r)?;
        let color = [r.u8()?, r.u8()?, r.u8()?, r.u8()?];
        let mut influence_bones = [0u8; MAX_INFLUENCES];
        let mut influence_weights = [0u8; MAX_INFLUENCES];
        for b in influence_bones.iter_mut() {
            *b = r.u8()?;
        }
        for x in influence_weights.iter_mut() {
            *x = r.u8()?;
        }
        Ok(SoftSkinVertex { position, tangent_x, tangent_y, tangent_z, uvs, color, influence_bones, influence_weights })
    }
    fn write(&self, w: &mut Writer) {
        write_vector(w, &self.position);
        w.u32(self.tangent_x);
        w.u32(self.tangent_y);
        w.u32(self.tangent_z);
        write_uvs(w, &self.uvs);
        w.bytes(&self.color);
        w.bytes(&self.influence_bones);
        w.bytes(&self.influence_weights);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkelMeshChunk {
    pub base_vertex_index: u32,
    pub rigid_vertices: Vec<RigidSkinVertex>,
    pub soft_vertices: Vec<SoftSkinVertex>,
    pub bone_map: Vec<u16>,
    pub num_rigid_vertices: i32,
    pub num_soft_vertices: i32,
    pub max_bone_influences: i32,
}

impl SkelMeshChunk {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(SkelMeshChunk {
            base_vertex_index: r.u32()?,
            rigid_vertices: r.array(RigidSkinVertex::read)?,
            soft_vertices: r.array(SoftSkinVertex::read)?,
            bone_map: r.array(|r| r.u16())?,
            num_rigid_vertices: r.i32()?,
            num_soft_vertices: r.i32()?,
            max_bone_influences: r.i32()?,
        })
    }
    fn write(&self, w: &mut Writer) {
        w.u32(self.base_vertex_index);
        w.array(&self.rigid_vertices, |w, v| v.write(w));
        w.array(&self.soft_vertices, |w, v| v.write(w));
        w.array(&self.bone_map, |w, b| w.u16(*b));
        w.i32(self.num_rigid_vertices);
        w.i32(self.num_soft_vertices);
        w.i32(self.max_bone_influences);
    }
}

/// `FSkeletalMeshVertexBuffer` (GPU skin): data absent in server packages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkeletalMeshVertexBuffer {
    pub num_tex_coords: u32,
    pub use_full_precision_uvs: i32,
    pub use_packed_position: i32,
    pub mesh_extension: Vector,
    pub mesh_origin: Vector,
    pub data: Option<BulkArray>,
}

/// `FSkeletalMeshVertexInfluences` (alternate weight sets).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VertexInfluences {
    pub influences: BulkArray,
    /// `TMap<FBoneIndexPair, TArray<DWORD>>`.
    pub mapping: Vec<([i32; 2], Vec<u32>)>,
    pub sections: Vec<SkelMeshSection>,
    pub chunks_raw: Vec<u8>,
    pub required_bones: Vec<u8>,
    pub custom_left_right_section_map: Vec<i32>,
    pub usage: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticLodModel {
    pub sections: Vec<SkelMeshSection>,
    pub index_container: MultiSizeIndexContainer,
    pub active_bone_indices: Vec<u16>,
    pub chunks: Vec<SkelMeshChunk>,
    pub size: i32,
    pub num_vertices: i32,
    pub required_bones: Vec<u8>,
    pub raw_point_indices: BulkData,
    pub num_tex_coords: i32,
    pub vertex_buffer: SkeletalMeshVertexBuffer,
    /// Present only when the mesh's `bHasVertexColors` is set; data absent on server.
    pub color_vb: Option<Option<BulkArray>>,
    pub vertex_influences: Vec<VertexInfluences>,
    pub adjacency_index_container: MultiSizeIndexContainer,
}

impl StaticLodModel {
    pub fn read(r: &mut Reader, layout: Layout, has_vertex_colors: bool) -> Result<Self> {
        let sections = r.array(SkelMeshSection::read)?;
        let index_container = MultiSizeIndexContainer::read(r)?;
        let active_bone_indices = r.array(|r| r.u16())?;
        let chunks = r.array(SkelMeshChunk::read)?;
        let size = r.i32()?;
        let num_vertices = r.i32()?;
        let required_bones = r.array(|r| r.u8())?;
        let raw_point_indices = BulkData::read(r)?;
        let num_tex_coords = r.i32()?;
        let mut vertex_buffer = SkeletalMeshVertexBuffer {
            num_tex_coords: r.u32()?,
            use_full_precision_uvs: r.i32()?,
            use_packed_position: r.i32()?,
            mesh_extension: read_vector(r)?,
            mesh_origin: read_vector(r)?,
            data: None,
        };
        if !layout.server {
            vertex_buffer.data = Some(BulkArray::read(r)?);
        }
        let color_vb = if has_vertex_colors { Some(if layout.server { None } else { Some(BulkArray::read(r)?) }) } else { None };
        let vertex_influences = r.array(|r| {
            let influences = BulkArray::read(r)?;
            let mapping = r.array(|r| Ok(([r.i32()?, r.i32()?], r.array(|r| r.u32())?)))?;
            let sections = r.array(SkelMeshSection::read)?;
            // Chunks of an influence set are written with the plain chunk layout.
            let start = r.pos();
            let n = r.i32()?;
            for _ in 0..n {
                SkelMeshChunk::read(r)?;
            }
            let chunks_raw = r.data()[start..r.pos()].to_vec();
            let required_bones = r.array(|r| r.u8())?;
            let custom_left_right_section_map = r.array(|r| r.i32())?;
            let usage = r.u8()?;
            Ok(VertexInfluences { influences, mapping, sections, chunks_raw, required_bones, custom_left_right_section_map, usage })
        })?;
        let adjacency_index_container = MultiSizeIndexContainer::read(r)?;
        Ok(StaticLodModel {
            sections,
            index_container,
            active_bone_indices,
            chunks,
            size,
            num_vertices,
            required_bones,
            raw_point_indices,
            num_tex_coords,
            vertex_buffer,
            color_vb,
            vertex_influences,
            adjacency_index_container,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.array(&self.sections, |w, s| s.write(w));
        self.index_container.write(w);
        w.array(&self.active_bone_indices, |w, b| w.u16(*b));
        w.array(&self.chunks, |w, c| c.write(w));
        w.i32(self.size);
        w.i32(self.num_vertices);
        w.array(&self.required_bones, |w, b| w.u8(*b));
        self.raw_point_indices.write(w);
        w.i32(self.num_tex_coords);
        let vb = &self.vertex_buffer;
        w.u32(vb.num_tex_coords);
        w.i32(vb.use_full_precision_uvs);
        w.i32(vb.use_packed_position);
        write_vector(w, &vb.mesh_extension);
        write_vector(w, &vb.mesh_origin);
        if let Some(d) = &vb.data {
            d.write(w);
        }
        if let Some(Some(d)) = &self.color_vb {
            d.write(w);
        }
        w.array(&self.vertex_influences, |w, vi| {
            vi.influences.write(w);
            w.array(&vi.mapping, |w, (pair, arr)| {
                w.i32(pair[0]);
                w.i32(pair[1]);
                w.array(arr, |w, x| w.u32(*x));
            });
            w.array(&vi.sections, |w, s| s.write(w));
            w.bytes(&vi.chunks_raw);
            w.array(&vi.required_bones, |w, b| w.u8(*b));
            w.array(&vi.custom_left_right_section_map, |w, x| w.i32(*x));
            w.u8(vi.usage);
        });
        self.adjacency_index_container.write(w);
    }
}

/// `FPerPolyBoneCollisionData`: kDOP tree + collision vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerPolyBoneCollision {
    pub kdop_tree: KDopTree,
    pub collision_verts: Vec<Vector>,
}

/// `USkeletalMesh::Serialize` at file version 893.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkeletalMesh {
    pub bounds: BoxSphereBounds,
    pub materials: Vec<PackageIndex>,
    pub origin: Vector,
    pub rot_origin: Rotator,
    pub ref_skeleton: Vec<MeshBone>,
    pub skeletal_depth: i32,
    pub lod_models: Vec<StaticLodModel>,
    /// `TMap<FName, INT> NameIndexMap`.
    pub name_index_map: Vec<(FName, i32)>,
    pub per_poly_bone_kdops: Vec<PerPolyBoneCollision>,
    pub bone_break_names: Vec<String>,
    pub bone_break_options: Vec<u8>,
    pub clothing_assets: Vec<PackageIndex>,
    pub cached_streaming_texture_factors: Vec<f32>,
    pub source_data: Option<Box<StaticLodModel>>,
}

impl SkeletalMesh {
    pub fn read(r: &mut Reader, layout: Layout, has_vertex_colors: bool) -> Result<Self> {
        let bounds = BoxSphereBounds::read(r)?;
        let materials = r.array(|r| r.index())?;
        let origin = read_vector(r)?;
        let rot_origin = [r.i32()?, r.i32()?, r.i32()?];
        let ref_skeleton = r.array(MeshBone::read)?;
        let skeletal_depth = r.i32()?;
        let lod_models = r.array(|r| StaticLodModel::read(r, layout, has_vertex_colors))?;
        let name_index_map = r.array(|r| Ok((r.fname()?, r.i32()?)))?;
        let per_poly_bone_kdops = r.array(|r| Ok(PerPolyBoneCollision { kdop_tree: KDopTree::read(r)?, collision_verts: r.array(read_vector)? }))?;
        let bone_break_names = r.array(|r| r.fstring())?;
        let bone_break_options = r.array(|r| r.u8())?;
        let clothing_assets = r.array(|r| r.index())?;
        let cached_streaming_texture_factors = r.array(|r| r.f32())?;
        let has_source = r.i32()?;
        let source_data = if has_source != 0 { Some(Box::new(StaticLodModel::read(r, layout, has_vertex_colors)?)) } else { None };
        Ok(SkeletalMesh {
            bounds,
            materials,
            origin,
            rot_origin,
            ref_skeleton,
            skeletal_depth,
            lod_models,
            name_index_map,
            per_poly_bone_kdops,
            bone_break_names,
            bone_break_options,
            clothing_assets,
            cached_streaming_texture_factors,
            source_data,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        self.bounds.write(w);
        w.array(&self.materials, |w, m| w.index(*m));
        write_vector(w, &self.origin);
        for x in self.rot_origin {
            w.i32(x);
        }
        w.array(&self.ref_skeleton, |w, b| b.write(w));
        w.i32(self.skeletal_depth);
        w.array(&self.lod_models, |w, m| m.write(w));
        w.array(&self.name_index_map, |w, (n, i)| {
            w.fname(*n);
            w.i32(*i);
        });
        w.array(&self.per_poly_bone_kdops, |w, p| {
            p.kdop_tree.write(w);
            w.array(&p.collision_verts, write_vector);
        });
        w.array(&self.bone_break_names, |w, s| w.fstring(s));
        w.array(&self.bone_break_options, |w, b| w.u8(*b));
        w.array(&self.clothing_assets, |w, c| w.index(*c));
        w.array(&self.cached_streaming_texture_factors, |w, f| w.f32(*f));
        w.i32(self.source_data.is_some() as i32);
        if let Some(s) = &self.source_data {
            s.write(w);
        }
    }
}

// ----------------------------------------------------------------------------
// Physics
// ----------------------------------------------------------------------------

/// `URB_BodySetup::Serialize`: `TArray<FKCachedConvexData> PreCachedPhysData`,
/// each a `TArray<FKCachedConvexDataElement>` of bulk-serialised PhysX
/// convex streams.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodySetup {
    pub pre_cached_phys_data: Vec<Vec<BulkArray>>,
}

impl BodySetup {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(BodySetup { pre_cached_phys_data: r.array(|r| r.array(BulkArray::read))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.pre_cached_phys_data, |w, d| w.array(d, |w, e| e.write(w)));
    }
}

/// `UPhysicsAssetInstance::Serialize`: `TMap<FRigidBodyIndexPair, UBOOL> CollisionDisableTable`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhysicsAssetInstance {
    pub collision_disable_table: Vec<([i32; 2], i32)>,
}

impl PhysicsAssetInstance {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(PhysicsAssetInstance { collision_disable_table: r.array(|r| Ok(([r.i32()?, r.i32()?], r.i32()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.collision_disable_table, |w, (p, b)| {
            w.i32(p[0]);
            w.i32(p[1]);
            w.i32(*b);
        });
    }
}
