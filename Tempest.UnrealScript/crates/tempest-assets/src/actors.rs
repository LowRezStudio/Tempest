//! Small native payloads of actors and components (UnInterpolation.cpp,
//! UnLight.cpp, UnPrefab.cpp, DecalComponent.cpp, InstancedFoliage.cpp,
//! FluidSurfaceComponent.cpp, UnNavigationMesh.cpp, SpeedTreeComponent.cpp,
//! UnCoreNative.cpp, NvApexGenericAsset.cpp).

use tempest_upk::io::{Error, FName, PackageIndex, Reader, Result, Writer};

use crate::level::LightMap;
use crate::mesh::{BoxSphereBounds, Vector};

fn read_vector(r: &mut Reader) -> Result<Vector> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}
fn write_vector(w: &mut Writer, v: &Vector) {
    for x in v {
        w.f32(*x);
    }
}
fn read_rotator(r: &mut Reader) -> Result<[i32; 3]> {
    Ok([r.i32()?, r.i32()?, r.i32()?])
}

/// `USeqAct_Interp::Serialize`: `TMap<AActor*, FSavedTransform>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeqActInterp {
    pub saved_actor_transforms: Vec<(PackageIndex, Vector, [i32; 3])>,
}

impl SeqActInterp {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(SeqActInterp { saved_actor_transforms: r.array(|r| Ok((r.index()?, read_vector(r)?, read_rotator(r)?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.saved_actor_transforms, |w, (a, l, rot)| {
            w.index(*a);
            write_vector(w, l);
            for x in rot {
                w.i32(*x);
            }
        });
    }
}

/// `AStaticLightCollectionActor::Serialize`: one `FMatrix` (light to world)
/// per entry of the `LightComponents` property; no count of its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticLightCollectionActor {
    pub light_to_world: Vec<[f32; 16]>,
}

impl StaticLightCollectionActor {
    pub fn read(r: &mut Reader) -> Result<Self> {
        if !r.remaining().is_multiple_of(64) {
            return Err(Error::Format("light collection payload is not a whole number of matrices".into()));
        }
        let mut light_to_world = Vec::with_capacity(r.remaining() / 64);
        while r.remaining() > 0 {
            let mut m = [0f32; 16];
            for x in m.iter_mut() {
                *x = r.f32()?;
            }
            light_to_world.push(m);
        }
        Ok(StaticLightCollectionActor { light_to_world })
    }
    pub fn write(&self, w: &mut Writer) {
        for m in &self.light_to_world {
            for x in m {
                w.f32(*x);
            }
        }
    }
}

/// `APrefabInstance::Serialize`: `ArchetypeToInstanceMap`, `PI_ObjectMap`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrefabInstance {
    pub archetype_to_instance: Vec<(PackageIndex, PackageIndex)>,
    pub object_map: Vec<(PackageIndex, i32)>,
}

impl PrefabInstance {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(PrefabInstance { archetype_to_instance: r.array(|r| Ok((r.index()?, r.index()?)))?, object_map: r.array(|r| Ok((r.index()?, r.i32()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.archetype_to_instance, |w, (a, b)| {
            w.index(*a);
            w.index(*b);
        });
        w.array(&self.object_map, |w, (a, b)| {
            w.index(*a);
            w.i32(*b);
        });
    }
}

/// `UDecalComponent::Serialize`: static receiver count (always 0 in
/// cooked packages; receivers are rebuilt at load).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecalComponent {
    pub num_static_receivers: i32,
}

impl DecalComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let n = r.i32()?;
        if n != 0 {
            return Err(Error::Format(format!("decal component with {n} static receivers is not supported")));
        }
        Ok(DecalComponent { num_static_receivers: n })
    }
    pub fn write(&self, w: &mut Writer) {
        w.i32(self.num_static_receivers);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoliageInstance {
    pub base: PackageIndex,
    pub location: Vector,
    pub rotation: [i32; 3],
    pub draw_scale_3d: Vector,
    pub cluster_index: i32,
    pub pre_align_rotation: [i32; 3],
    pub flags: u32,
    pub z_offset: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoliageInstanceCluster {
    pub bounds: BoxSphereBounds,
    pub cluster_component: PackageIndex,
    pub instance_indices: Vec<i32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoliageMeshInfo {
    pub instance_clusters: Vec<FoliageInstanceCluster>,
    pub instances: Vec<FoliageInstance>,
    pub settings: PackageIndex,
}

/// `AInstancedFoliageActor::Serialize`: `TMap<UStaticMesh*, FFoliageMeshInfo>`,
/// then (Hi-Rez, when `StreamingSerializationVersion == 1` on a non-template
/// actor) a `StreamGroups` array, kept as raw bytes (always empty in the
/// shipped maps).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstancedFoliageActor {
    pub foliage_meshes: Vec<(PackageIndex, FoliageMeshInfo)>,
    pub stream_groups: Vec<u8>,
}

impl InstancedFoliageActor {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let foliage_meshes = r.array(|r| {
            let mesh = r.index()?;
            let instance_clusters = r.array(|r| Ok(FoliageInstanceCluster { bounds: BoxSphereBounds::read(r)?, cluster_component: r.index()?, instance_indices: r.array(|r| r.i32())? }))?;
            let instances = r.array(|r| {
                Ok(FoliageInstance {
                    base: r.index()?,
                    location: read_vector(r)?,
                    rotation: read_rotator(r)?,
                    draw_scale_3d: read_vector(r)?,
                    cluster_index: r.i32()?,
                    pre_align_rotation: read_rotator(r)?,
                    flags: r.u32()?,
                    z_offset: r.f32()?,
                })
            })?;
            let settings = r.index()?;
            Ok((mesh, FoliageMeshInfo { instance_clusters, instances, settings }))
        })?;
        let stream_groups = r.take(r.remaining())?.to_vec();
        if stream_groups.len() > 4 || (stream_groups.len() == 4 && stream_groups != [0, 0, 0, 0]) {
            return Err(Error::Format("foliage stream groups with entries are not supported".into()));
        }
        Ok(InstancedFoliageActor { foliage_meshes, stream_groups })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.foliage_meshes, |w, (mesh, info)| {
            w.index(*mesh);
            w.array(&info.instance_clusters, |w, c| {
                c.bounds.write(w);
                w.index(c.cluster_component);
                w.array(&c.instance_indices, |w, i| w.i32(*i));
            });
            w.array(&info.instances, |w, i| {
                w.index(i.base);
                write_vector(w, &i.location);
                for x in i.rotation {
                    w.i32(x);
                }
                write_vector(w, &i.draw_scale_3d);
                w.i32(i.cluster_index);
                for x in i.pre_align_rotation {
                    w.i32(x);
                }
                w.u32(i.flags);
                w.f32(i.z_offset);
            });
            w.index(info.settings);
        });
        w.bytes(&self.stream_groups);
    }
}

/// `UFluidSurfaceComponent::Serialize`: one light map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FluidSurfaceComponent {
    pub light_map: LightMap,
}

impl FluidSurfaceComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(FluidSurfaceComponent { light_map: LightMap::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.light_map.write(w);
    }
}

/// `USpeedTreeComponent::Serialize`: branch, frond, leaf card, billboard
/// and leaf mesh light maps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeedTreeComponent {
    pub light_maps: Vec<LightMap>,
}

impl SpeedTreeComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let mut light_maps = Vec::with_capacity(5);
        for _ in 0..5 {
            light_maps.push(LightMap::read(r)?);
        }
        Ok(SpeedTreeComponent { light_maps })
    }
    pub fn write(&self, w: &mut Writer) {
        for l in &self.light_maps {
            l.write(w);
        }
    }
}

/// `APylon::Serialize`: `NavMeshPtr`, `ObstacleMesh`, and
/// `DynamicObstacleMesh` only when it was non-null (no marker on disk).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pylon {
    pub nav_mesh: PackageIndex,
    pub obstacle_mesh: PackageIndex,
    pub dynamic_obstacle_mesh: Option<PackageIndex>,
}

impl Pylon {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let nav_mesh = r.index()?;
        let obstacle_mesh = r.index()?;
        let dynamic_obstacle_mesh = if r.remaining() >= 4 { Some(r.index()?) } else { None };
        Ok(Pylon { nav_mesh, obstacle_mesh, dynamic_obstacle_mesh })
    }
    pub fn write(&self, w: &mut Writer) {
        w.index(self.nav_mesh);
        w.index(self.obstacle_mesh);
        if let Some(d) = self.dynamic_obstacle_mesh {
            w.index(d);
        }
    }
}

/// `UMetaData::Serialize`: `TMap<UObject*, TMap<FName, FString>>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetaData {
    pub objects: Vec<(PackageIndex, Vec<(FName, String)>)>,
}

impl MetaData {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(MetaData { objects: r.array(|r| Ok((r.index()?, r.array(|r| Ok((r.fname()?, r.fstring()?)))?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.objects, |w, (o, m)| {
            w.index(*o);
            w.array(m, |w, (k, v)| {
                w.fname(*k);
                w.fstring(v);
            });
        });
    }
}

/// `UApexGenericAsset::Serialize`: valid flag, asset name buffer, APEX
/// asset stream.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApexGenericAsset {
    pub valid: u32,
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}

impl ApexGenericAsset {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let valid = r.u32()?;
        if valid == 0 {
            return Ok(ApexGenericAsset::default());
        }
        let n = r.u32()? as usize;
        let name = r.take(n)?.to_vec();
        let m = r.u32()? as usize;
        let data = r.take(m)?.to_vec();
        Ok(ApexGenericAsset { valid, name, data })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.valid);
        if self.valid != 0 {
            w.u32(self.name.len() as u32);
            w.bytes(&self.name);
            w.u32(self.data.len() as u32);
            w.bytes(&self.data);
        }
    }
}

/// Hi-Rez `AHavokNavMeshActor::Serialize` / `UHavokNavMeshGrid::Serialize`:
/// one bulk data member holding the Havok AI stream (8.2 server binary).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HavokNavMesh {
    pub data: crate::bulk::BulkData,
}

impl HavokNavMesh {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(HavokNavMesh { data: crate::bulk::BulkData::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.data.write(w);
    }
}

/// `USoundClass::Serialize`: `TMap<USoundNode*, FSoundNodeEditorData>` (empty when cooked).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundClass {
    pub editor_data: Vec<(PackageIndex, i32, i32)>,
}

impl SoundClass {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(SoundClass { editor_data: r.array(|r| Ok((r.index()?, r.i32()?, r.i32()?)))? })
    }
    pub fn write(&self, w: &mut Writer) {
        w.array(&self.editor_data, |w, (n, x, y)| {
            w.index(*n);
            w.i32(*x);
            w.i32(*y);
        });
    }
}

/// `ULandscapeComponent::Serialize` (PC): one light map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LandscapeComponent {
    pub light_map: LightMap,
}

impl LandscapeComponent {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(LandscapeComponent { light_map: LightMap::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.light_map.write(w);
    }
}

/// `ULandscapeHeightfieldCollisionComponent::Serialize`: height and
/// dominant-layer bulk data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LandscapeCollision {
    pub collision_height_data: crate::bulk::BulkData,
    pub dominant_layer_data: crate::bulk::BulkData,
}

impl LandscapeCollision {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(LandscapeCollision { collision_height_data: crate::bulk::BulkData::read(r)?, dominant_layer_data: crate::bulk::BulkData::read(r)? })
    }
    pub fn write(&self, w: &mut Writer) {
        self.collision_height_data.write(w);
        self.dominant_layer_data.write(w);
    }
}
