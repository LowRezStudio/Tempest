//! Text rendering of parsed objects (`upk dump`).

use std::fmt::Write as _;

use tempest_upk::flags::{describe, OBJECT_FLAG_NAMES};
use tempest_upk::io::{FName, PackageIndex};
use tempest_upk::Package;

use crate::bulk::BulkData;
use crate::object::{Native, Object, Parsed};
use crate::texture::Mip;
use crate::value::{Property, StructValue, Value};

/// Bytes of a byte array written out (as hex rows); the rest is counted.
const DUMP_BYTES_SHOWN: usize = 4096;
/// Elements of any other array written out.
const DUMP_ITEMS_SHOWN: usize = 2000;

pub struct Dumper<'a> {
    pub pkg: &'a Package,
}

impl<'a> Dumper<'a> {
    pub fn new(pkg: &'a Package) -> Dumper<'a> {
        Dumper { pkg }
    }

    pub fn name(&self, n: FName) -> String {
        self.pkg.fname_to_string(n)
    }

    /// `Class'Package.Outer.Name'` for a reference.
    pub fn object_ref(&self, idx: PackageIndex) -> String {
        if idx.is_null() {
            return "None".to_string();
        }
        let class = self.pkg.class_name_of(idx);
        let path = self.pkg.object_path(idx);
        format!("{class}'{path}'")
    }

    pub fn export_line(&self, i: usize) -> String {
        let e = &self.pkg.exports[i];
        let idx = PackageIndex::export(i);
        format!(
            "{i:6}  {} {}  size={} flags={}",
            self.pkg.class_name_of(idx),
            self.pkg.object_path(idx),
            e.serial_size,
            describe(e.object_flags, OBJECT_FLAG_NAMES)
        )
    }

    pub fn parsed(&self, p: &Parsed, out: &mut String) {
        match p {
            Parsed::Empty => out.push_str("  (empty placeholder)\n"),
            Parsed::Raw { data, error } => {
                let _ = writeln!(out, "  (unparsed: {error}; {} bytes)", data.len());
            }
            Parsed::Object(o) => self.object(o, out),
            Parsed::Script(o) => {
                let _ = writeln!(out, "  net index: {}", o.header.net_index);
                let _ = writeln!(out, "  script object: {} tagged properties, body {:?}", o.header.properties.len(), std::mem::discriminant(&o.body));
            }
        }
    }

    pub fn object(&self, o: &Object, out: &mut String) {
        if !o.native_prefix.is_empty() {
            let _ = writeln!(out, "  native prefix: {} bytes", o.native_prefix.len());
        }
        if let Some(sf) = &o.state_frame {
            let _ = writeln!(
                out,
                "  state frame: node={} state={} probe={:#x} latent={} stack={} code={:?}",
                self.object_ref(sf.node),
                self.object_ref(sf.state_node),
                sf.probe_mask,
                sf.latent_action,
                sf.state_stack.len(),
                sf.code_offset
            );
        }
        if let Some(c) = &o.component {
            let _ = writeln!(
                out,
                "  component: template owner class={} template name={}",
                self.object_ref(c.template_owner_class),
                c.template_name.map(|n| self.name(n)).unwrap_or_else(|| "-".into())
            );
        }
        let _ = writeln!(out, "  net index: {}", o.net_index);
        for p in &o.properties {
            self.property(p, 1, out);
        }
        self.native(&o.native, out);
    }

    /// The class-specific payload, one summary block.
    pub fn native(&self, native: &Native, out: &mut String) {
        match native {
            Native::Raw(b) if b.is_empty() => {}
            Native::Raw(b) => {
                let _ = writeln!(out, "  native data: {} bytes (not decoded)", b.len());
                let n = b.len().min(64);
                let hex: Vec<String> = b[..n].iter().map(|x| format!("{x:02x}")).collect();
                let _ = writeln!(out, "    {}{}", hex.join(" "), if b.len() > n { " …" } else { "" });
            }
            Native::Texture2D(t) => {
                let _ = writeln!(out, "  texture2d: source art {}; file cache guid {}", bulk_text(&t.source_art), t.file_cache_guid);
                mips_text("mips", &t.mips, out);
                if !t.cached_pvrtc_mips.is_empty() {
                    mips_text("cached pvrtc mips", &t.cached_pvrtc_mips, out);
                }
                if !t.cached_atitc_mips.is_empty() {
                    mips_text("cached atitc mips", &t.cached_atitc_mips, out);
                }
                if !t.cached_etc_mips.is_empty() {
                    mips_text("cached etc mips", &t.cached_etc_mips, out);
                }
                let _ = writeln!(out, "    flash: max res {} {}", t.cached_flash_mips_max_resolution, bulk_text(&t.cached_flash_mips));
                if let Some(f) = t.lightmap_flags {
                    let _ = writeln!(out, "    lightmap flags: {f:#x}");
                }
            }
            Native::Texture(t) => {
                let _ = writeln!(out, "  texture: source art {}", bulk_text(&t.source_art));
            }
            Native::TextureMovie(t) => {
                let _ = writeln!(out, "  texture movie: source art {}; data {}", bulk_text(&t.source_art), bulk_text(&t.data));
            }
            Native::SoundNodeWave(t) => {
                for (name, b) in crate::misc::SOUND_WAVE_BULKS.iter().zip(&t.bulks) {
                    if b.element_count != 0 || b.inline.as_ref().map(|d| !d.is_empty()).unwrap_or(false) {
                        let _ = writeln!(out, "  sound {name}: {}", bulk_text(b));
                    }
                }
            }
            Native::SoundCue(t) => {
                let _ = writeln!(out, "  sound cue editor data: {} entries", t.editor_data.len());
            }
            Native::Font(t) => {
                let _ = writeln!(out, "  font char remap: {} entries", t.char_remap.len());
            }
            Native::ObjectRedirector(t) => {
                let _ = writeln!(out, "  redirects to: {}", self.object_ref(t.destination));
            }
            Native::GuidCache(t) => {
                let _ = writeln!(out, "  guid cache: {} packages", t.packages.len());
                for (n, g) in t.packages.iter().take(8) {
                    let _ = writeln!(out, "    {} = {g}", self.name(*n));
                }
                if t.packages.len() > 8 {
                    let _ = writeln!(out, "    …");
                }
            }
            Native::ShadowMap1D(t) => {
                let _ = writeln!(out, "  shadow map 1d: {} samples, light {}", t.samples.len(), t.light_guid);
            }
            Native::StaticMesh(m) => {
                let _ = writeln!(out, "  static mesh: {} LODs, body setup {}, kdop nodes {} ({} bytes) tris {} ({} bytes), bounds origin {:?} extent {:?} radius {}, source data {}, internal version {}", m.lod_models.len(), self.object_ref(m.body_setup), m.kdop_tree.nodes.count, m.kdop_tree.nodes.elem_size, m.kdop_tree.triangles.count, m.kdop_tree.triangles.elem_size, m.bounds.origin, m.bounds.box_extent, m.bounds.sphere_radius, m.source_data.is_some(), m.internal_version);
                for (i, l) in m.lod_models.iter().enumerate() {
                    let _ = writeln!(out, "    lod {i}: {} verts, {} indices ({}-byte), {} sections, uv sets {} (stride {}, full uvs {}), colors {}, raw triangles {}, wireframe {} adjacency {}{}", l.num_vertices, l.index_buffer.count, l.index_buffer.elem_size, l.elements.len(), l.vertex_buffer.num_tex_coords, l.vertex_buffer.stride, l.vertex_buffer.use_full_precision_uvs, l.color_vb.num_vertices, bulk_text(&l.raw_triangles), l.wireframe_index_buffer.count, l.adjacency_index_buffer.count, if l.vertex_buffer.data.is_none() { " (server: no GPU data)" } else { "" });
                    for (j, e) in l.elements.iter().enumerate() {
                        let _ = writeln!(out, "      section {j}: {} tris from index {}, verts {}..{}, material {} (slot {}), collision {}, shadow {}, fragments {:?}", e.num_triangles, e.first_index, e.min_vertex_index, e.max_vertex_index, self.object_ref(e.material), e.material_index, e.enable_collision, e.enable_shadow_casting, e.fragments);
                    }
                }
            }
            Native::SkeletalMesh(m) => {
                let _ = writeln!(out, "  skeletal mesh: {} bones, {} materials, {} LODs, bounds origin {:?} extent {:?} radius {}, per-poly kdops {}, source data {}", m.ref_skeleton.len(), m.materials.len(), m.lod_models.len(), m.bounds.origin, m.bounds.box_extent, m.bounds.sphere_radius, m.per_poly_bone_kdops.len(), m.source_data.is_some());
                for (i, l) in m.lod_models.iter().enumerate() {
                    let vb = &l.vertex_buffer;
                    let _ = writeln!(out, "    lod {i}: {} verts, {} indices ({}-byte, cpu {}), {} sections, {} chunks, size {}, uv sets {} (full {}, packed {}, stride {}), origin {:?} ext {:?}, required bones {}, active bones {}, raw points {}, colors {}, influence sets {}, adjacency {}{}", l.num_vertices, l.index_container.indices.count, l.index_container.data_type_size, l.index_container.needs_cpu_access, l.sections.len(), l.chunks.len(), l.size, vb.num_tex_coords, vb.use_full_precision_uvs, vb.use_packed_position, vb.data.as_ref().map(|d| d.elem_size).unwrap_or(0), vb.mesh_origin, vb.mesh_extension, l.required_bones.len(), l.active_bone_indices.len(), bulk_text(&l.raw_point_indices), l.color_vb.as_ref().map(|c| c.as_ref().map(|d| d.count).unwrap_or(-1)).unwrap_or(-2), l.vertex_influences.len(), l.adjacency_index_container.indices.count, if vb.data.is_none() { " (server: no GPU data)" } else { "" });
                    for (j, s) in l.sections.iter().enumerate() {
                        let _ = writeln!(out, "      section {j}: {} tris from index {}, material slot {}, chunk {}, sorting {}", s.num_triangles, s.base_index, s.material_index, s.chunk_index, s.triangle_sorting);
                    }
                    for (j, c) in l.chunks.iter().enumerate() {
                        let _ = writeln!(out, "      chunk {j}: base vertex {}, {} rigid + {} soft (arrays {} + {}), {} bones in map, max influences {}", c.base_vertex_index, c.num_rigid_vertices, c.num_soft_vertices, c.rigid_vertices.len(), c.soft_vertices.len(), c.bone_map.len(), c.max_bone_influences);
                    }
                }
            }
            Native::BodySetup(b) => {
                let _ = writeln!(out, "  body setup: {} cached convex sets", b.pre_cached_phys_data.len());
            }
            Native::PhysicsAssetInstance(p) => {
                let _ = writeln!(out, "  physics asset instance: {} disabled collision pairs", p.collision_disable_table.len());
            }
            Native::AnimSequence(a) => {
                let _ = writeln!(out, "  anim sequence: {} raw tracks, {} bytes compressed", a.raw_tracks.len(), a.compressed.len());
            }
            Native::StaticMeshComponent(c) => {
                let _ = writeln!(out, "  static mesh component: {} LOD entries", c.lod_data.len());
                for (i, l) in c.lod_data.iter().enumerate() {
                    let lm = match &l.light_map {
                        crate::level::LightMap::None => "none".to_string(),
                        crate::level::LightMap::OneD { directional_samples, .. } => format!("1D ({} samples)", directional_samples.element_count),
                        crate::level::LightMap::TwoD { textures, .. } => format!("2D ({})", textures.iter().map(|(t, _)| self.object_ref(*t)).collect::<Vec<_>>().join(", ")),
                    };
                    let _ = writeln!(out, "    lod {i}: light map {lm}, {} shadow maps, {} painted vertices{}", l.shadow_maps.len(), l.painted_vertices.len(), if l.override_vertex_colors.is_some() { ", vertex colours overridden" } else { "" });
                }
            }
            Native::BrushComponent(b) => {
                let _ = writeln!(out, "  brush component: {} cached convex hulls", b.cached_phys_brush_data.len());
            }
            Native::ModelComponent(m) => {
                let _ = writeln!(out, "  model component: model {}, zone {}, {} elements, {} nodes", self.object_ref(m.model), m.zone_index, m.elements.len(), m.nodes.len());
            }
            Native::Model(m) => {
                let _ = writeln!(out, "  bsp model: {} points, {} vectors, {} nodes, {} surfs, {} verts, {} zones, polys {}", m.points.count, m.vectors.count, m.nodes.count, m.surfs.len(), m.verts.count, m.zones.len(), self.object_ref(m.polys));
            }
            Native::Polys(p) => {
                let _ = writeln!(out, "  polys: {} polygons, owner {}", p.elements.len(), self.object_ref(p.owner));
            }
            Native::World(wd) => {
                let _ = writeln!(out, "  world: persistent level {}, {} extra referenced objects", self.object_ref(wd.persistent_level), wd.extra_referenced_objects.len());
            }
            Native::Material(m) => {
                let _ = writeln!(out, "  material: quality mask {:#x}, {} resources", m.quality_mask, m.resources.len());
                for (q, res) in &m.resources {
                    let _ = writeln!(out, "    quality {q}: id {}, {} textures, {} lookups, {} user uvs{}", res.id, res.uniform_expression_textures.len(), res.texture_lookups.len(), res.num_user_tex_coords, if res.compile_errors.is_empty() { String::new() } else { format!(", {} compile errors", res.compile_errors.len()) });
                }
            }
            Native::MaterialInstance(m) => {
                if m.permutations.is_empty() {
                    let _ = writeln!(out, "  material instance: no static permutation");
                } else {
                    let _ = writeln!(out, "  material instance: quality mask {:#x}, {} static permutations", m.quality_mask, m.permutations.len());
                    for (q, res, params) in &m.permutations {
                        let _ = writeln!(out, "    quality {q}: id {}, base {}, {} switches, {} masks, {} normals, {} textures", res.id, params.base_material_id, params.static_switch_parameters.len(), params.static_component_mask_parameters.len(), params.normal_parameters.len(), res.uniform_expression_textures.len());
                    }
                }
            }
            Native::SeqActInterp(t) => {
                let _ = writeln!(out, "  matinee: {} saved actor transforms", t.saved_actor_transforms.len());
            }
            Native::StaticLightCollectionActor(t) => {
                let _ = writeln!(out, "  static light collection: {} light transforms", t.light_to_world.len());
            }
            Native::PrefabInstance(t) => {
                let _ = writeln!(out, "  prefab instance: {} archetype mappings, {} object mappings", t.archetype_to_instance.len(), t.object_map.len());
            }
            Native::DecalComponent(t) => {
                let _ = writeln!(out, "  decal component: {} static receivers", t.num_static_receivers);
            }
            Native::InstancedFoliageActor(t) => {
                let _ = writeln!(out, "  instanced foliage: {} meshes, {} instances", t.foliage_meshes.len(), t.foliage_meshes.iter().map(|(_, m)| m.instances.len()).sum::<usize>());
            }
            Native::FluidSurfaceComponent(_) | Native::SpeedTreeComponent(_) => {
                let _ = writeln!(out, "  light maps decoded");
            }
            Native::Pylon(t) => {
                let _ = writeln!(out, "  pylon: nav mesh {}, obstacle mesh {}", self.object_ref(t.nav_mesh), self.object_ref(t.obstacle_mesh));
            }
            Native::MetaData(t) => {
                let _ = writeln!(out, "  metadata: {} objects", t.objects.len());
            }
            Native::ApexGenericAsset(t) => {
                let _ = writeln!(out, "  apex asset: {} bytes", t.data.len());
            }
            Native::HavokNavMesh(t) => {
                let _ = writeln!(out, "  havok nav mesh: {}", bulk_text(&t.data));
            }
            Native::SoundClass(t) => {
                let _ = writeln!(out, "  sound class editor data: {} entries", t.editor_data.len());
            }
            Native::LandscapeComponent(_) => {
                let _ = writeln!(out, "  landscape component: light map decoded");
            }
            Native::LandscapeCollision(t) => {
                let _ = writeln!(out, "  landscape collision: heights {}, layers {}", bulk_text(&t.collision_height_data), bulk_text(&t.dominant_layer_data));
            }
            Native::Level(l) => {
                let _ = writeln!(out, "  level: {} actors, map \"{}\", model {}, {} model components, {} streamed textures, {} light volume samples, {} visibility buckets", l.actors.len(), l.url.map, self.object_ref(l.model), l.model_components.len(), l.texture_to_instances.len(), l.precomputed_light_volume.samples.len(), l.precomputed_visibility.buckets.len());
            }
        }
    }

    pub fn property(&self, p: &Property, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        let mut key = self.name(p.name);
        if p.array_index > 0 {
            key = format!("{key}[{}]", p.array_index);
        }
        match &p.value {
            Value::Struct(StructValue::Tagged(ps)) => {
                let sn = p.struct_name.map(|n| self.name(n)).unwrap_or_default();
                let _ = writeln!(out, "{indent}{key} ({sn}) {{");
                for q in ps {
                    self.property(q, depth + 1, out);
                }
                let _ = writeln!(out, "{indent}}}");
            }
            Value::Struct(StructValue::Binary(ms)) => {
                let sn = p.struct_name.map(|n| self.name(n)).unwrap_or_default();
                let _ = writeln!(out, "{indent}{key} ({sn}) {{");
                for m in ms {
                    self.member_line(m, depth + 1, out);
                }
                let _ = writeln!(out, "{indent}}}");
            }
            Value::Array(items) => self.array(&key, items, depth, out),
            v => {
                let _ = writeln!(out, "{indent}{key} = {}", self.scalar(v));
            }
        }
    }

    fn member_line(&self, m: &crate::value::Member, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        if m.elems.len() == 1 {
            self.indexed_value(&m.name, &m.elems[0], depth, out);
        } else {
            let _ = writeln!(out, "{indent}{} [{}]", m.name, m.elems.len());
            for (i, e) in m.elems.iter().enumerate() {
                self.indexed_value(&format!("[{i}]"), e, depth + 1, out);
            }
        }
    }

    fn indexed_value(&self, key: &str, v: &Value, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        match v {
            Value::Struct(StructValue::Tagged(ps)) => {
                let _ = writeln!(out, "{indent}{key} {{");
                for q in ps {
                    self.property(q, depth + 1, out);
                }
                let _ = writeln!(out, "{indent}}}");
            }
            Value::Struct(StructValue::Binary(ms)) => {
                let _ = writeln!(out, "{indent}{key} {{");
                for m in ms {
                    self.member_line(m, depth + 1, out);
                }
                let _ = writeln!(out, "{indent}}}");
            }
            Value::Array(items) => self.array(key, items, depth, out),
            v => {
                let _ = writeln!(out, "{indent}{key} = {}", self.scalar(v));
            }
        }
    }

    /// An array, cut short when long: a movie's or a sound's bytes are megabytes, and one
    /// line per element made a dump of hundreds of megabytes (the Studio's Dump tab hung).
    fn array(&self, key: &str, items: &[Value], depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        let _ = writeln!(out, "{indent}{key} [{}]", items.len());
        if !items.is_empty() && items.iter().all(|v| matches!(v, Value::Byte(_))) {
            // Bytes as hex rows.
            let shown = items.len().min(DUMP_BYTES_SHOWN);
            for (row, chunk) in items[..shown].chunks(32).enumerate() {
                let hex: Vec<String> = chunk
                    .iter()
                    .map(|v| match v {
                        Value::Byte(b) => format!("{b:02x}"),
                        _ => unreachable!(),
                    })
                    .collect();
                let _ = writeln!(out, "{indent}  {:08x}  {}", row * 32, hex.join(" "));
            }
            if items.len() > shown {
                let _ = writeln!(out, "{indent}  … {} more bytes", items.len() - shown);
            }
            return;
        }
        let shown = items.len().min(DUMP_ITEMS_SHOWN);
        for (i, it) in items[..shown].iter().enumerate() {
            self.indexed_value(&format!("[{i}]"), it, depth + 1, out);
        }
        if items.len() > shown {
            let _ = writeln!(out, "{indent}  … {} more elements", items.len() - shown);
        }
    }

    pub fn scalar(&self, v: &Value) -> String {
        match v {
            Value::Bool(b) => if *b { "True".into() } else { "False".into() },
            Value::Byte(b) => b.to_string(),
            Value::Enum(n) => self.name(*n),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format!("{f}"),
            Value::Name(n) => format!("'{}'", self.name(*n)),
            Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Object(i) => self.object_ref(*i),
            Value::Delegate { object, function } => format!("{}.{}", self.object_ref(*object), self.name(*function)),
            Value::Raw(b) => {
                let n = b.len().min(32);
                let hex: Vec<String> = b[..n].iter().map(|x| format!("{x:02x}")).collect();
                format!("<raw {} bytes: {}{}>", b.len(), hex.join(" "), if b.len() > n { " …" } else { "" })
            }
            Value::Struct(_) | Value::Array(_) => "<compound>".into(),
        }
    }
}

/// One-line description of a bulk data header.
pub fn bulk_text(b: &BulkData) -> String {
    let mut f = Vec::new();
    if b.is_separate() { f.push("separate"); }
    if b.flags & crate::bulk::flags::MERGED_FILE != 0 { f.push("merged-file"); }
    if b.is_unused() { f.push("unused"); }
    if b.flags & crate::bulk::flags::SERIALIZE_COMPRESSED_LZO != 0 { f.push("lzo"); }
    if b.flags & crate::bulk::flags::SERIALIZE_COMPRESSED_ZLIB != 0 { f.push("zlib"); }
    if b.flags & crate::bulk::flags::SERIALIZE_COMPRESSED_LZX != 0 { f.push("lzx"); }
    if b.inline.is_some() { f.push("inline"); }
    format!("[flags={:#x} {} count={} disk={} offset={}]", b.flags, f.join(","), b.element_count, b.size_on_disk, b.offset_in_file)
}

fn mips_text(label: &str, mips: &[Mip], out: &mut String) {
    let _ = writeln!(out, "    {label} [{}]", mips.len());
    for (i, m) in mips.iter().enumerate() {
        let _ = writeln!(out, "      [{i}] {}x{} {}", m.size_x, m.size_y, bulk_text(&m.data));
    }
}
