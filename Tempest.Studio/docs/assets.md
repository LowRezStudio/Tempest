# Studio packages and asset editing

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- Previews: textures (PNG of any mip, zoom, background), materials and material instances
  (`material.preview`: lit sphere/cube/plane with the resolved base colour, normal and emissive
  maps, plus the parent chain, every texture with a thumbnail, scalar/vector/static-switch
  parameters, nearest override wins; scalars and vectors are editable: `material.setParameter`
  writes an override entry on an instance, with the base material's expression GUID when it is in
  the same package, or the expression's `DefaultValue` on a material), meshes (three.js viewer:
  orbit, LOD, textures on/off, wireframe, skeleton, grid, background, Fit), sounds (audio player).
  The mesh viewer takes the `mesh.preview` GLB (textures resolved through the material chain; mask
  templates are never used as emissive maps, cube maps are left out, a texture the mesh's material
  imports from a package that is not a file is found through the search index, map meshes without
  materials take them from the first placed component, and a cooked material without expressions
  falls back to its compiled texture list). Mesh **Export** writes glTF (.glb, metres; each joint is
  rotated so its +Y points at the child that continues its chain, with the inverse bind matrices
  absorbing it, so Blender draws limbs instead of sideways sticks; the skin is unchanged), FBX
  (.fbx, binary 7.4, centimetres; `mesh_fbx.rs`, textures embedded, skeleton + skin) or OBJ;
  warnings list textures that could not be embedded.

- **Sounds**: a SoundNodeWave's preview has a player and an **Edit** button: waveform (decoded by
  Web Audio at the stored rate), drag to select, trim to selection, fade in/out, gain, normalise,
  mono/stereo, Vorbis quality, play the result, then **Apply and replace** sends the edited 16-bit
  PCM to `sound.replacePcm`, which encodes Ogg Vorbis (libvorbis aoTuV through `vorbis_rs`,
  `audio.rs`) into the package (undoable, saved with the package). Replace accepts .ogg (stored as
  is), .wav and .wem (re-encoded).

- **Wwise** (`wwise/` in tempest-assets): AkEvent and AkBank exports preview what they play: the
  bank name (the event's `RequiredBank`) names the `<bank>.pck` in the workspace folder, the event
  name's FNV-1 hash finds the event in the bank's `HIRC`, play actions are followed through
  containers to sounds/music tracks and their media ids; every media file gets Play (Wwise Vorbis
  converted to Ogg on the fly), Export (.ogg/.wav/.wem), Replace (.ogg/.wav/.wem, re-encoded to
  Wwise Vorbis unless already a .wem) and Edit (the same editor, `wwise.replacePcm`). Replaced media
  are an unsaved edit of the `.pck` (`PckEdit` in `serve.rs`: the parsed package and the replaced
  media by id, kept until `package.save`, which `serve.rs` routes to `pck_call` for `.pck` paths
  like `package.discard|close|dirty|undo|undoList`): the renderer marks the `.pck` dirty like a
  package, Save writes it (recording it in `basestore` first, `record_pck`: the hashes of its
  entries, bank objects and media, `wwise::delta::digest`, so it is listed as saved in place and
  gets a delta), workspace files keep only the replaced media (`kind: "pck"` entries, applied to the
  game's file on load, `twrk.apply` returns them in `pcks`), and the cooker lists it as `kind:
  "pck"`: a delta like a package's (`wwise/delta.rs`; see [delta
  format](../../Tempest.UnrealScript/docs/deltas.md)), or the whole file when chosen or when the
  original is unknown (a bare `{kind: "pck"}` mark from an older Studio). The toolbar's **Wwise
  audio** dialog browses every `.pck` of the workspace (events named from the AkEvent exports,
  selecting one filters the media). Not covered: non-Vorbis codecs (none in this game), batch import
  of Wwise media, loop points.

- **Particles** (`particle.rs`, `particle.system` / `particle.setTable`): a ParticleSystem, emitter,
  LOD level or module export previews the whole system in a three.js CPU simulation
  (`src/lib/studio/particles.ts`): sprite emitters as instanced camera-facing quads with the
  material's base texture, blend mode and sub-UV sheets, mesh emitters as instanced meshes, the
  common modules (spawn rate and bursts, lifetime, size, size/colour over life, velocity,
  acceleration, location shapes, rotation, orbit, axis lock); beams, ribbons, collision and events
  are listed as "not simulated". Cooked systems keep no curve objects, only the baked
  `FRawDistribution` lookup tables the game samples: the side panel shows a module's distributions
  in a curve editor (drag points, resample, time scale, start time, numeric table) that updates the
  preview live and, on Apply, writes the table into the module (undoable, saved with the package).
  Modules only store what differs from their class defaults, so the backend merges the class default
  object's distributions (walking the class chain) into what the Studio receives; in the file an
  absent table means the default.

- **New packages and adding objects**: **New package…** (`NewPackageDialog.svelte`, package list
  `+`, `package.create {path, template}`) makes an empty package in memory with the header settings
  of a template package (`Package::clear_contents`, fresh GUID): listed with a `new` badge
  (`workspace.packages` adds unsaved new editors), unsaved like any edit, kept whole in workspace
  files (`new: true`, recreated on load), saved with Save (then recorded by `basestore::record_new`)
  and shipped whole by the cooker (`base.kind: "new"`). **Add to the package…**
  (`AddObjectDialog.svelte`, export tree `+`, a group's or object's context menu; `ui.addGroup`): a
  new object from a file (texture, mesh, sound, movie, or a raw `.bin` payload; `object.add {path,
  file, template, name, outer}`: the template, an object of the same kind from any package picked
  with `ObjectPicker.svelte`, is copied with `CopyMode::Import` (its own sub-objects copied,
  everything else it uses referenced in its package by import), renamed, then imported with
  `batch::import_file`), a copy from another package (`object.copy`), or a group (`object.addGroup`,
  `Editor::add_group`: a `Core.Package` export with the cooker's 12-byte payload).
  `workspace.search` takes `isA` (a class and its subclasses).

- **Mesh materials**: the mesh preview lists the material slots (`MeshMaterials.svelte`,
  `mesh.materials`: skeletal `Materials`, static mesh sections by `MaterialIndex` across LODs; map
  props have none, their components carry them): open one (another package's through
  `studio.openObjectPath`, found by the search index), **Change…** (`mesh.setMaterial`, any
  `MaterialInterface`, referenced by import when elsewhere: `Editor::import_for_export`), **Make it
  this mesh's own** (`mesh.ownMaterial`: the instance copied next to the mesh as `<mesh>_<material>`
  and assigned; base materials are refused). Skeletal meshes: **Add slot** (`mesh.addSlot`, the last
  slot's material) and remove a slot no section draws with (`mesh.removeSlot`, later slots move
  down). Every skeletal change goes through `mesh_import::conform_skeletal`:
  `LODInfo[l].bEnableShadowCasting` / `TriangleSortSettings` get one entry per section of LOD l (the
  engine indexes them by section: a mesh imported with more sections than these hold crashed the
  game), `LODMaterialMap` values stay valid slots, `ClothingAssets` one entry per slot. Mesh import
  gives a skeletal mesh a new slot for each of the file's materials past its last one (named matches
  first, then the file's distinct materials in order) and clears the imported LOD's
  `LODMaterialMap`. The material preview edits texture parameters (`material.setTexture`,
  `ParamValue::Texture`: an instance override in `TextureParameterValues`, or a material's
  `TextureSampleParameter` `Texture`): **Change…** to any texture, or **From a file…** (`object.add`
  modelled on the current texture, next to the material, then assigned).

- **Property editing**: array properties get add (copy of the last element; an empty array gets an
  element at its default), duplicate and remove element buttons, and properties and tagged-struct
  fields a remove (back to default) button (`object.list`); **Add a property…** and a tagged
  struct's **+** (`AddPropertyPicker.svelte`, `object.missing` lists what the class or struct
  declares and the object does not store, filtered like the cooker serialises; `object.addProperty`,
  `Editor::add_property`) add one at its default: scalars from the class default objects
  (`ClassDb::cdo_scalar`), enums at their first value, structs binary in cooked packages
  (LinearColor, Vector, Guid…) as all members at default (`default_value`), other structs empty;
  copies and duplicates get NetIndex -1 (`Editor::clear_net_index`); **Duplicate** (inspector,
  Ctrl+D) copies an export next to it as `…_copy` with the same payload (`object.duplicate`, no
  sub-objects).

- **Animations** (`AnimPreview.svelte`, `anim.info|preview|export|import|files|edit`, `anim.rs` +
  `anim_ops.rs`): an AnimSequence plays on a skeletal mesh (the AnimSet's `PreviewSkelMeshName`,
  found anywhere in the workspace through the search index, else the package's meshes ranked by
  bones in common; a picker switches) with play/pause, frame scrubbing, speed, loop and skeleton;
  its notifies sit on a timeline (drag to move, right-click to copy at the playhead, move there or
  remove; undoable), and "Edit the sequence" trims to a frame range, retimes to a new length
  (notifies follow) and sets `RateScale`. An AnimSet previews too (a picker walks its sequences,
  `anim.info` resolves the set to its first sequence and lists `sequences`). Export
  (`AnimExportDialog.svelte`) writes the mesh, skeleton and animation as glTF or FBX (baked keys per
  frame), with a checkbox for every sequence of the AnimSet (one animation each, named by
  `SequenceName`; on by default for an AnimSet; `anim.export {all}`); Replace… (Animation file tab)
  takes a glTF/FBX animation (several in a file: pick one), matches bones by name, resamples at the
  sequence's rate, re-encodes and rescales the notifies to the new length, or, with the "every
  sequence" box (always on for an AnimSet; `anim.import {all}`), puts each animation of the file
  into the sequence of the same name (`SequenceName`, else object name, `Armature|` prefixes
  ignored; the dialog previews the pairs) as one undo step and reports the unmatched ones.

- **Replace…** on any export: textures (PNG/DDS), meshes (glTF/GLB/FBX/OBJ, static and skeletal,
  `mesh_import.rs` + `mesh_fbx.rs`), animations (glTF/GLB/FBX), sounds (Ogg/WAV/wem), movies
  (GFX/SWF), and for everything the raw serialized payload (`object.replaceRaw`, the inverse of
  Extract raw; `upk inject` on the CLI). **From another package** (`object.replaceWith`,
  `Editor::replace_with`, `upk obj replace-with`): the export becomes a copy of an object of the
  same class in another package, references remapped like Copy to (deep by default: what it uses in
  its own package is copied along, objects already present at the same path reused; else referenced
  there by import), keeping its name, outer, archetype and NetIndex; e.g. a match skeletal mesh
  replaced by the lobby one with its three material slots (a raw payload from another package refers
  to that package's tables and breaks).

- **Replace** (Ctrl+H, `package.findReplace`): case-insensitive substring find over every string,
  name, enum and object-reference value and optionally object names of the open package, preview
  table, then replace all (renames first, references re-resolved from the replaced path). **Batch
  import** (`batch.plan` / `batch.run`): a folder of `<package>.<object>.<ext>` files, each resolved
  to its package and object (full path or unique name among objects the file type can replace; when
  `<package>` is not a package file of the workspace, or the object is not in it, the whole
  `<package>.<object>` is an object path looked up in every package through the search index and the
  file goes to each package holding it, as cooked seekfree packages keep content under top-level
  groups like `loadingart_zhin`), imported by type into the open packages as unsaved edits (one undo
  step per package, `batch.run`; the renderer marks them dirty), saved, kept in the project and
  cooked like any other edit; when the plan touches several packages the dialog lists them with
  checkboxes (all selected, Select all / Deselect all) and `batch.run` takes `packages` (the package
  files to change), the others' entries reported as "package not selected". CLI: `upk replace`, `upk
  batch-import` (those save the packages).

- Script exports (classes, functions, states, structs, enums, properties) get a **Source** tab: the
  owning class is decompiled (`script.source`), edited in CodeMirror, and **Compile**
  (`script.compile`) rebuilds the whole package from the decompiled sources with the edited class
  swapped in, conformed to the file on disk exactly like `uscript project build`; the result
  replaces the open package in memory and Save writes it. Compiling needs a saved package (no
  pending property edits). Saving a script package in place reloads the class database.
