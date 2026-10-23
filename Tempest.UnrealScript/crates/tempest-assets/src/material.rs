//! Materials: `UMaterial::Serialize` / `UMaterialInstance::Serialize`
//! (Material.cpp, MaterialInstance.cpp) with `FMaterialResource::Serialize`
//! and `FStaticParameterSet::Serialize` (MaterialShared.cpp). The 8.2
//! binary matches the 10897 source; at file version 893 uniform expressions
//! live in the shader cache, so a material resource is just its compile
//! state, texture references and usage flags.

use tempest_upk::io::{FName, Guid, PackageIndex, Reader, Result, Writer};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureLookup {
    pub tex_coord_index: i32,
    pub texture_index: i32,
    pub u_scale: f32,
    pub v_scale: f32,
}

/// `FMaterialResource::Serialize` (`FMaterial::Serialize` + blend override).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialResource {
    pub compile_errors: Vec<String>,
    /// `TMap<UMaterialExpression*, INT>`.
    pub texture_dependency_length_map: Vec<(PackageIndex, i32)>,
    pub max_texture_dependency_length: i32,
    pub id: Guid,
    pub num_user_tex_coords: u32,
    pub uniform_expression_textures: Vec<PackageIndex>,
    pub uses_scene_color: i32,
    pub uses_scene_depth: i32,
    pub uses_dynamic_parameter: i32,
    pub uses_lightmap_uvs: i32,
    pub uses_material_vertex_position_offset: i32,
    pub using_transforms: u32,
    pub texture_lookups: Vec<TextureLookup>,
    pub dropped_fallback_components: u32,
    pub blend_mode_override_value: i32,
    pub is_blend_mode_overrided: i32,
    pub is_masked_override_value: i32,
}

impl MaterialResource {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(MaterialResource {
            compile_errors: r.array(|r| r.fstring())?,
            texture_dependency_length_map: r.array(|r| Ok((r.index()?, r.i32()?)))?,
            max_texture_dependency_length: r.i32()?,
            id: r.guid()?,
            num_user_tex_coords: r.u32()?,
            uniform_expression_textures: r.array(|r| r.index())?,
            uses_scene_color: r.i32()?,
            uses_scene_depth: r.i32()?,
            uses_dynamic_parameter: r.i32()?,
            uses_lightmap_uvs: r.i32()?,
            uses_material_vertex_position_offset: r.i32()?,
            using_transforms: r.u32()?,
            texture_lookups: r.array(|r| Ok(TextureLookup { tex_coord_index: r.i32()?, texture_index: r.i32()?, u_scale: r.f32()?, v_scale: r.f32()? }))?,
            dropped_fallback_components: r.u32()?,
            blend_mode_override_value: r.i32()?,
            is_blend_mode_overrided: r.i32()?,
            is_masked_override_value: r.i32()?,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.array(&self.compile_errors, |w, s| w.fstring(s));
        w.array(&self.texture_dependency_length_map, |w, (i, n)| {
            w.index(*i);
            w.i32(*n);
        });
        w.i32(self.max_texture_dependency_length);
        w.guid(&self.id);
        w.u32(self.num_user_tex_coords);
        w.array(&self.uniform_expression_textures, |w, i| w.index(*i));
        w.i32(self.uses_scene_color);
        w.i32(self.uses_scene_depth);
        w.i32(self.uses_dynamic_parameter);
        w.i32(self.uses_lightmap_uvs);
        w.i32(self.uses_material_vertex_position_offset);
        w.u32(self.using_transforms);
        w.array(&self.texture_lookups, |w, t| {
            w.i32(t.tex_coord_index);
            w.i32(t.texture_index);
            w.f32(t.u_scale);
            w.f32(t.v_scale);
        });
        w.u32(self.dropped_fallback_components);
        w.i32(self.blend_mode_override_value);
        w.i32(self.is_blend_mode_overrided);
        w.i32(self.is_masked_override_value);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticSwitchParameter {
    pub name: FName,
    pub value: i32,
    pub override_: i32,
    pub expression_guid: Guid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticComponentMaskParameter {
    pub name: FName,
    pub r: i32,
    pub g: i32,
    pub b: i32,
    pub a: i32,
    pub override_: i32,
    pub expression_guid: Guid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NormalParameter {
    pub name: FName,
    pub compression_settings: u8,
    pub override_: i32,
    pub expression_guid: Guid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainLayerWeightParameter {
    pub name: FName,
    pub weightmap_index: i32,
    pub override_: i32,
    pub expression_guid: Guid,
}

/// `FStaticParameterSet::Serialize`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StaticParameterSet {
    pub base_material_id: Guid,
    pub static_switch_parameters: Vec<StaticSwitchParameter>,
    pub static_component_mask_parameters: Vec<StaticComponentMaskParameter>,
    pub normal_parameters: Vec<NormalParameter>,
    pub terrain_layer_weight_parameters: Vec<TerrainLayerWeightParameter>,
}

impl StaticParameterSet {
    pub fn read(r: &mut Reader) -> Result<Self> {
        Ok(StaticParameterSet {
            base_material_id: r.guid()?,
            static_switch_parameters: r.array(|r| Ok(StaticSwitchParameter { name: r.fname()?, value: r.i32()?, override_: r.i32()?, expression_guid: r.guid()? }))?,
            static_component_mask_parameters: r.array(|r| {
                Ok(StaticComponentMaskParameter { name: r.fname()?, r: r.i32()?, g: r.i32()?, b: r.i32()?, a: r.i32()?, override_: r.i32()?, expression_guid: r.guid()? })
            })?,
            normal_parameters: r.array(|r| Ok(NormalParameter { name: r.fname()?, compression_settings: r.u8()?, override_: r.i32()?, expression_guid: r.guid()? }))?,
            terrain_layer_weight_parameters: r.array(|r| Ok(TerrainLayerWeightParameter { name: r.fname()?, weightmap_index: r.i32()?, override_: r.i32()?, expression_guid: r.guid()? }))?,
        })
    }

    pub fn write(&self, w: &mut Writer) {
        w.guid(&self.base_material_id);
        w.array(&self.static_switch_parameters, |w, p| {
            w.fname(p.name);
            w.i32(p.value);
            w.i32(p.override_);
            w.guid(&p.expression_guid);
        });
        w.array(&self.static_component_mask_parameters, |w, p| {
            w.fname(p.name);
            w.i32(p.r);
            w.i32(p.g);
            w.i32(p.b);
            w.i32(p.a);
            w.i32(p.override_);
            w.guid(&p.expression_guid);
        });
        w.array(&self.normal_parameters, |w, p| {
            w.fname(p.name);
            w.u8(p.compression_settings);
            w.i32(p.override_);
            w.guid(&p.expression_guid);
        });
        w.array(&self.terrain_layer_weight_parameters, |w, p| {
            w.fname(p.name);
            w.i32(p.weightmap_index);
            w.i32(p.override_);
            w.guid(&p.expression_guid);
        });
    }
}

/// Hi-Rez's build has three material quality levels (the 10897 source has
/// two): quality mask 7 is the norm in shipped packages.
pub const MSQ_MAX: u32 = 3;

/// `UMaterial::Serialize` (also `UDecalMaterial`): a quality mask and one
/// resource per set bit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    pub quality_mask: u32,
    pub resources: Vec<(u32, MaterialResource)>,
}

impl Material {
    pub fn read(r: &mut Reader) -> Result<Self> {
        let quality_mask = r.u32()?;
        let mut resources = Vec::new();
        for q in 0..MSQ_MAX {
            if quality_mask & (1 << q) != 0 {
                resources.push((q, MaterialResource::read(r)?));
            }
        }
        Ok(Material { quality_mask, resources })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.quality_mask);
        for (_, res) in &self.resources {
            res.write(w);
        }
    }
}

/// `UMaterialInstance::Serialize`: nothing unless the instance has a static
/// permutation (`bHasStaticPermutationResource` property), then a quality
/// mask and, per set bit, a resource and its static parameter set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaterialInstance {
    pub quality_mask: u32,
    pub permutations: Vec<(u32, MaterialResource, StaticParameterSet)>,
}

impl MaterialInstance {
    pub fn read(r: &mut Reader, has_static_permutation: bool) -> Result<Self> {
        if !has_static_permutation {
            return Ok(MaterialInstance::default());
        }
        let quality_mask = r.u32()?;
        let mut permutations = Vec::new();
        for q in 0..MSQ_MAX {
            if quality_mask & (1 << q) != 0 {
                let res = MaterialResource::read(r)?;
                let params = StaticParameterSet::read(r)?;
                permutations.push((q, res, params));
            }
        }
        Ok(MaterialInstance { quality_mask, permutations })
    }
    pub fn write(&self, w: &mut Writer) {
        if self.permutations.is_empty() && self.quality_mask == 0 {
            return;
        }
        w.u32(self.quality_mask);
        for (_, res, params) in &self.permutations {
            res.write(w);
            params.write(w);
        }
    }
}
