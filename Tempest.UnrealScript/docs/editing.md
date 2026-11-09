# UnrealScript: editing

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Editing (`edit.rs`, `visit.rs`)

`Editor` parses exports on demand and writes back only touched ones, saving with the original
compression/obscure settings.

- `obj delete`: the export (and its sub-objects) become the cooker's zero-size, class-less `none`
  placeholders — no index shifting — and every reference to them in tables and payloads is cleared
  (`visit.rs` reaches every `PackageIndex`/`FName` in every `Native`; keep it in sync when adding
  serializers).
- `obj copy`: copies the dependency closure from the source package (sub-objects, materials,
  textures, package groups), reusing destination objects that already exist at the same path,
  turning script objects (classes, functions) into imports, interning names, and setting `NetIndex`
  to -1 on copies (not replicated; the cooker's per-package NetIndex numbering is not reproduced).
  `--shallow` clears references to objects the destination lacks instead.
- `prop set`: paths are `Name`, `Name[i]` (static array slot or dynamic element), `Outer.Member`,
  `Array[i].Member`; values parse by the property kind (ints, floats, `True`/`False`, enum tags,
  `'Name'`, `"text"`, `Class'Path'` or `None` for objects, which must already exist in the package).
  Missing properties are created from the class reflection.
- `mod-dir`: writes `manifest.toml` + `files/<pkg>` for `tempest mod pack` / `just mod-pack`
  (signing stays in the .NET CLI).

glTF export (`mesh_ops.rs`, `material_ops.rs`): materials resolve `TextureParameterValues` up the
`Parent` chain, then the base material's `MaterialExpressionTextureSample*` sub-objects; slots are
classified by Hi-Rez's parameter names (`diffuse`, `normal`, `specular`, `emissive`, `rgbmask`),
other textures land in `extras.textures`. Textures are embedded as PNG (largest available mip,
cross-package references opened from the cooked dir; an import whose root is not a package file,
e.g. `pc_mastermaterial.…` cooked into a `*_SF` package, goes through `PackageCache`'s path lookup:
a lazy scan of every package's export table on the CLI, the Studio's search index in `serve`; what
still cannot be found is reported as a warning instead of silently dropped). Cube maps and other
non-`Texture2D` classes are skipped. A cooked `Material` without `MaterialExpressionTextureSample*`
sub-objects (map packages strip them) falls back to the compiled resource's
`UniformExpressionTextures`, classified by texture name; a mesh whose sections have no material (map
props: cooked maps set them on the components) takes the `Materials[]` of the first `MeshComponent`
using it. An emissive map identical to the base colour is dropped. Skeletal meshes get a skin: one
node per bone with its local matrix (`FQuat`+position, column-major, converted with the y/z swap),
inverse bind matrices from the bind pose, `JOINTS_0`/`WEIGHTS_0` from the GPU skin influences
(bone-map resolved per chunk). FBX export and import live in `mesh_fbx.rs`.

Deliberately still raw: `ShaderCache` (compiled shader bytecode, `FShaderCache::Load`; 1.3 GB on
8.1, nothing to edit), `TgPersistentCookerData` (one cooker bookkeeping export, no symbols in the
binary), and in the dev server / 0.57 sets only: terrain, decal components with static receivers,
landscape components. Coverage: 8.1 client 99.93 % of exports fully typed, 8.2 server 100.00 %, 0.57
99.96 %.

Container rules learned here: offsets in a compressed package are in the coordinates of its
uncompressed form (the summary is laid out without the chunk table, which `compress_image` adds
afterwards); zero-size placeholder exports have `SerialOffset` 0; `probe_layout` falls back to
`Class` exports for script packages with no property exports (server `BattleGame`).

### Copying and creating objects (`edit.rs`)

`Editor::copy_with(src, i, outer, mode)`: `CopyMode::Deep` copies every referenced export of the
source package, `Clear` clears references to objects the destination lacks, `Import` references them
in the source package (`import_for_export`, outer chain included); the copied object's own
sub-objects (outer chain reaching it) always come along. `copy_from(deep)` is Deep/Clear.
`add_group(name, outer)` writes a `Core.Package` export with the cooker's group payload (net index
−1, `None`). A new package (`package.create` in `serve.rs`) is a loaded template package after
`Package::clear_contents()` (tables, payloads and the private name lookup emptied; the summary kept)
with a fresh GUID; the writer recomputes the header.
