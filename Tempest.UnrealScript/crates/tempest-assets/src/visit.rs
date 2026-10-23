//! Walks every `FName` and object reference inside a parsed object, so the
//! editor can remap them when objects move between packages or are removed.

use tempest_upk::io::{FName, PackageIndex};

use crate::level::LightMap;
use crate::object::{Native, Object};
use crate::value::{Property, StructValue, Value};

pub trait Visitor {
    fn name(&mut self, n: &mut FName);
    fn index(&mut self, i: &mut PackageIndex);
}

/// Something that holds names and references.
pub trait Visit {
    fn visit(&mut self, v: &mut dyn Visitor);
}

impl Visit for Value {
    fn visit(&mut self, v: &mut dyn Visitor) {
        match self {
            Value::Enum(n) | Value::Name(n) => v.name(n),
            Value::Object(i) => v.index(i),
            Value::Delegate { object, function } => {
                v.index(object);
                v.name(function);
            }
            Value::Struct(StructValue::Tagged(ps)) => ps.iter_mut().for_each(|p| p.visit(v)),
            Value::Struct(StructValue::Binary(ms)) => ms.iter_mut().for_each(|m| m.elems.iter_mut().for_each(|e| e.visit(v))),
            Value::Array(items) => items.iter_mut().for_each(|e| e.visit(v)),
            Value::Bool(_) | Value::Byte(_) | Value::Int(_) | Value::Float(_) | Value::Str(_) | Value::Raw(_) => {}
        }
    }
}

impl Visit for Property {
    fn visit(&mut self, v: &mut dyn Visitor) {
        v.name(&mut self.name);
        v.name(&mut self.type_name);
        if let Some(s) = &mut self.struct_name {
            v.name(s);
        }
        if let Some(e) = &mut self.enum_name {
            v.name(e);
        }
        self.value.visit(v);
    }
}

impl Visit for LightMap {
    fn visit(&mut self, v: &mut dyn Visitor) {
        match self {
            LightMap::None => {}
            LightMap::OneD { owner, .. } => v.index(owner),
            LightMap::TwoD { textures, .. } => textures.iter_mut().for_each(|(t, _)| v.index(t)),
        }
    }
}

impl Visit for Native {
    fn visit(&mut self, v: &mut dyn Visitor) {
        match self {
            Native::Raw(_)
            | Native::Texture2D(_)
            | Native::Texture(_)
            | Native::TextureMovie(_)
            | Native::SoundNodeWave(_)
            | Native::Font(_)
            | Native::ShadowMap1D(_)
            | Native::BodySetup(_)
            | Native::PhysicsAssetInstance(_)
            | Native::AnimSequence(_)
            | Native::BrushComponent(_)
            | Native::StaticLightCollectionActor(_)
            | Native::DecalComponent(_)
            | Native::ApexGenericAsset(_)
            | Native::HavokNavMesh(_)
            | Native::LandscapeCollision(_) => {}
            Native::SoundCue(c) => c.editor_data.iter_mut().for_each(|(i, _, _)| v.index(i)),
            Native::SoundClass(c) => c.editor_data.iter_mut().for_each(|(i, _, _)| v.index(i)),
            Native::ObjectRedirector(r) => v.index(&mut r.destination),
            Native::GuidCache(g) => g.packages.iter_mut().for_each(|(n, _)| v.name(n)),
            Native::StaticMesh(m) => {
                v.index(&mut m.body_setup);
                for lod in m.lod_models.iter_mut().chain(m.source_data.iter_mut().map(|b| &mut **b)) {
                    lod.elements.iter_mut().for_each(|e| v.index(&mut e.material));
                }
            }
            Native::SkeletalMesh(m) => {
                m.materials.iter_mut().for_each(|i| v.index(i));
                m.ref_skeleton.iter_mut().for_each(|b| v.name(&mut b.name));
                m.name_index_map.iter_mut().for_each(|(n, _)| v.name(n));
                m.clothing_assets.iter_mut().for_each(|i| v.index(i));
            }
            Native::StaticMeshComponent(c) => {
                for l in &mut c.lod_data {
                    l.shadow_maps.iter_mut().for_each(|i| v.index(i));
                    l.shadow_vertex_buffers.iter_mut().for_each(|i| v.index(i));
                    l.light_map.visit(v);
                }
            }
            Native::ModelComponent(c) => {
                v.index(&mut c.model);
                for e in &mut c.elements {
                    e.light_map.visit(v);
                    v.index(&mut e.component);
                    v.index(&mut e.material);
                    e.shadow_maps.iter_mut().for_each(|i| v.index(i));
                }
            }
            Native::Model(m) => {
                v.index(&mut m.surfs_owner);
                for s in &mut m.surfs {
                    v.index(&mut s.material);
                    v.index(&mut s.actor);
                }
                m.zones.iter_mut().for_each(|z| v.index(&mut z.zone_actor));
                v.index(&mut m.polys);
            }
            Native::Polys(p) => {
                v.index(&mut p.owner);
                for e in &mut p.elements {
                    v.index(&mut e.actor);
                    v.name(&mut e.item_name);
                    v.index(&mut e.material);
                    v.name(&mut e.ruleset_variation);
                }
            }
            Native::World(w) => {
                v.index(&mut w.persistent_level);
                v.index(&mut w.persistent_facefx_anim_set);
                v.index(&mut w.save_game_summary);
                w.extra_referenced_objects.iter_mut().for_each(|i| v.index(i));
            }
            Native::Level(l) => {
                v.index(&mut l.actors_owner);
                l.actors.iter_mut().for_each(|i| v.index(i));
                v.index(&mut l.model);
                l.model_components.iter_mut().for_each(|i| v.index(i));
                l.game_sequences.iter_mut().for_each(|i| v.index(i));
                l.texture_to_instances.iter_mut().for_each(|(t, _)| v.index(t));
                for (c, insts) in &mut l.dynamic_texture_instances {
                    v.index(c);
                    insts.iter_mut().for_each(|d| v.index(&mut d.texture));
                }
                l.cached_phys_sm_data_map.iter_mut().for_each(|(m, _)| v.index(m));
                l.cached_phys_per_tri_sm_data_map.iter_mut().for_each(|(m, _)| v.index(m));
                l.force_stream_textures.iter_mut().for_each(|(t, _)| v.index(t));
                for p in [&mut l.nav_list, &mut l.cover_list, &mut l.pylon_list] {
                    v.index(&mut p.0);
                    v.index(&mut p.1);
                }
                l.cover_link_refs.iter_mut().for_each(|i| v.index(i));
                l.cross_level_actors.iter_mut().for_each(|i| v.index(i));
            }
            Native::Material(m) => {
                for (_, r) in &mut m.resources {
                    r.texture_dependency_length_map.iter_mut().for_each(|(i, _)| v.index(i));
                    r.uniform_expression_textures.iter_mut().for_each(|i| v.index(i));
                }
            }
            Native::MaterialInstance(m) => {
                for (_, r, p) in &mut m.permutations {
                    r.texture_dependency_length_map.iter_mut().for_each(|(i, _)| v.index(i));
                    r.uniform_expression_textures.iter_mut().for_each(|i| v.index(i));
                    p.static_switch_parameters.iter_mut().for_each(|s| v.name(&mut s.name));
                    p.static_component_mask_parameters.iter_mut().for_each(|s| v.name(&mut s.name));
                    p.normal_parameters.iter_mut().for_each(|s| v.name(&mut s.name));
                    p.terrain_layer_weight_parameters.iter_mut().for_each(|s| v.name(&mut s.name));
                }
            }
            Native::SeqActInterp(s) => s.saved_actor_transforms.iter_mut().for_each(|(a, _, _)| v.index(a)),
            Native::PrefabInstance(p) => {
                for (a, b) in &mut p.archetype_to_instance {
                    v.index(a);
                    v.index(b);
                }
                p.object_map.iter_mut().for_each(|(a, _)| v.index(a));
            }
            Native::InstancedFoliageActor(f) => {
                for (mesh, info) in &mut f.foliage_meshes {
                    v.index(mesh);
                    info.instance_clusters.iter_mut().for_each(|c| v.index(&mut c.cluster_component));
                    info.instances.iter_mut().for_each(|i| v.index(&mut i.base));
                    v.index(&mut info.settings);
                }
            }
            Native::FluidSurfaceComponent(f) => f.light_map.visit(v),
            Native::SpeedTreeComponent(s) => s.light_maps.iter_mut().for_each(|l| l.visit(v)),
            Native::LandscapeComponent(l) => l.light_map.visit(v),
            Native::Pylon(p) => {
                v.index(&mut p.nav_mesh);
                v.index(&mut p.obstacle_mesh);
                if let Some(d) = &mut p.dynamic_obstacle_mesh {
                    v.index(d);
                }
            }
            Native::MetaData(m) => {
                for (o, kv) in &mut m.objects {
                    v.index(o);
                    kv.iter_mut().for_each(|(k, _)| v.name(k));
                }
            }
        }
    }
}

impl Visit for Object {
    fn visit(&mut self, v: &mut dyn Visitor) {
        if let Some(sf) = &mut self.state_frame {
            v.index(&mut sf.node);
            v.index(&mut sf.state_node);
            for s in &mut sf.state_stack {
                v.index(&mut s.state);
                v.index(&mut s.node);
            }
        }
        if let Some(c) = &mut self.component {
            v.index(&mut c.template_owner_class);
            if let Some(n) = &mut c.template_name {
                v.name(n);
            }
        }
        self.properties.iter_mut().for_each(|p| p.visit(v));
        self.native.visit(v);
    }
}

/// Collect every object reference of an object.
pub fn references(obj: &mut Object) -> Vec<PackageIndex> {
    struct C(Vec<PackageIndex>);
    impl Visitor for C {
        fn name(&mut self, _: &mut FName) {}
        fn index(&mut self, i: &mut PackageIndex) {
            if !i.is_null() {
                self.0.push(*i);
            }
        }
    }
    let mut c = C(Vec::new());
    obj.visit(&mut c);
    c.0.sort_by_key(|i| i.0);
    c.0.dedup();
    c.0
}
