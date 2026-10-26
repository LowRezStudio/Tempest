# AGENTS.md — Tempest.UnrealScript

Rust toolchain for Paladins packages (Unreal Engine 3, build 10897, package file version 893): the UnrealScript compiler/decompiler (`uscript`) and the asset layer (`upk`). The one rule that matters: **output must be byte-identical to what the game's own compiler, cooker and `SavePackage` produce.** Every change is judged by the round-trip harnesses below, not by whether the result "looks right".

## What it is for

- Compile UnrealScript mods without UDK, against the game's own cooked packages.
- Decompile every script package of a game install into a UDK-style `Development/Src/<Pkg>/Classes/*.uc` tree, edit, and recompile everything.
- Read, extract, edit and insert the assets of any game package (`tempest-assets`, `upk`); the GUI editor builds on it.
- Later: merge/squash packages, server (`--server`) layouts.

## Layout

Cargo workspace, three crates:

```
crates/tempest-upk/            UPK container: summary, name/import/export tables, LZO, cooker-style chunking
  src/package.rs               Package::load / write, read_summary, parse_header, add_import/add_export, compress_image
                               (FArchiveSaveCompressed::CompressArchive: header chunk absorbs exports up to 1 MiB, then ≤1 MiB groups)
  src/file.rs                  PackageFile: memory-mapped, lazily decompressed read-only view (chunk cache, Sync)
  src/tfc.rs                   Texture file caches: Tfc (mmap reader), TfcWriter (append raw / compressed chunk)
  src/io.rs                    FArchive-like cursor, FName, PackageIndex
  src/flags.rs                 cpf / func / cf / rf / pkg flag constants (incl. Hi-Rez cpf::SAFE_OBJECT)
  src/lzo.rs                   LZO1X compressor/decompressor

crates/tempest-assets/         Typed object layer over any package (see "Asset layer" below)
  src/reflect.rs               ClassDb: the game's script packages (PKG_ContainsScript) loaded into a Context, class/struct index,
                               INTRINSIC_CLASSES (native-only classes such as LightMapTexture2D, Model, ShaderCache → parent)
  src/value.rs                 Property / Value: typed FPropertyTag values, decode (reflection) + exact encode
  src/object.rs                parse_export / write_object: UObject header + properties + Native (per-class payload, Raw until a serializer exists)
  src/bulk.rs                  FUntypedBulkData header + inline payload (cooked layout, Hi-Rez BULKDATA_MergedFile = 0x100)
  src/texture.rs               Texture2D / LightMapTexture2D / TextureCube / TextureMovie native payloads (exact read + write)
  src/mergedindex.rs           MergedFileIndexCache.bin: MD5(upper-case path) → mip → (size, offset, size on disk, tfc GUID), tfc names
  src/texcache.rs              TextureSource: resolves every mip to inline bytes or a `.tfc` offset and expands it
  src/dds.rs                   EPixelFormat ↔ DDS, DXT1/3/5/BC5/A8R8G8B8/G8/V8U8 decode, encode (texpresso), mip generation, PNG
  src/texture_ops.rs           texture_info / export_dds / export_png / import (replace pixels, all mips inline)
  src/misc.rs                  ObjectRedirector, GuidCache, SoundCue, Font/MultiFont (CharRemap), ShadowMap1D, SoundNodeWave (9 bulk members)
  src/mesh.rs                  StaticMesh (kDOP, render data, vertex/index buffers), SkeletalMesh (bones, LOD models, chunks, GPU skin buffer),
                               RB_BodySetup (cached PhysX convex streams), PhysicsAssetInstance; client vs server `Layout`
  src/mesh_ops.rs              Geometry extraction (positions/normals/UVs/indices/sections/influences) and glTF (.glb) / OBJ writers
  src/fbx.rs                   Binary FBX container (7.1–7.7 node tree, zlib arrays): parse + write
  src/mesh_fbx.rs              Mesh ⇄ FBX 7.4 (geometry, materials with embedded PNGs, LimbNode skeleton, Skin/Cluster weights, bind pose)
  src/audio.rs                 PCM in/out: WAV read/write, Ogg Vorbis encode/decode (vorbis_rs = libvorbis aoTuV + Lancer, built from C by cc), editor ops
  src/wwise/                   Audiokinetic Wwise as the game ships it (bank version 120): akpk.rs (`.pck` packages: language map, bank/stream/external
                               tables, byte-identical rewrite), bnk.rs (soundbank chunks, DIDX/DATA media replace with HIRC in-memory sizes patched,
                               event → action → containers → sounds/music tracks → media ids), vorbis.rs (Wwise Vorbis `.wem` ⇄ Ogg: packed setup with
                               10-bit ids into the aoTuV 6.03 codebook library (data/packed_codebooks_aoTuV_603.bin, ww2ogg's), mod packets, the 66-byte
                               AkVorbisInfo header incl. FNV-1 setup hash and the decoder allocation sizes copied from the game's files), fnv.rs
  src/particle.rs              Cooked particle systems: the baked FRawDistribution tables (`[min, max]` + chunked entries, time scale/start),
                               `set_distribution` (edit a module's table in place), the system owning an export
  src/mesh_import.rs           glTF/GLB/FBX/OBJ → mesh: rebuilds static LOD buffers + bounds + compact kDOP tree (TkDOPTreeCompact::Build port),
                               skeletal GPU skin buffer / chunks (≤75 bones) / sections / bones / bounds (UnMeshEd.cpp rule); skeleton kept
  src/level.rs                 AnimSequence, FLightMap (1D/2D), StaticMeshComponent LODData, BrushComponent, ModelComponent, Model, Polys, World, Level
  src/material.rs              Material / MaterialInstance resources (FMaterialResource, FStaticParameterSet; 3 quality levels)
  src/actors.rs                SeqAct_Interp, StaticLightCollectionActor, PrefabInstance, DecalComponent, InstancedFoliageActor, FluidSurface/SpeedTree
                               components, Pylon, MetaData, ApexGenericAsset, HavokNavMesh{Actor,Grid}, SoundClass, Landscape components
  src/material_ops.rs          PackageCache (cross-package import resolution) + material texture resolution (instance overrides → parent → expressions)
  src/visit.rs                 Visitor over every FName / object reference of an object (header, properties, every Native)
  src/edit.rs                  Editor: rename, delete (placeholder + reference clearing), cross-package copy (dependency closure), property get/set by path,
                               undo journal (begin_undo / snapshot / undo: per-export snapshots, table truncation, summary), save
  src/batch.rs                 Find & replace over a package (values + object names) and folder batch import (`<package>.<object>.<ext>`; a first part that is not a package file is a group, the whole name is then an object path found through a `PathLookup` in every package)
  src/media.rs                 SwfMovie RawData export/import, SoundNodeWave Ogg export/import (+Duration/NumChannels/SampleRate), property setters
  src/dump.rs                  Text rendering (`upk dump`)
  src/verify.rs                Directory round-trip + coverage report
  src/bin/upk.rs               CLI: info, ls, dump, extract, decompress, compress, verify, imagediff, texture {info,export,export-all,import}

crates/tempest-unrealscript/
  src/objects.rs               Serialised forms of UField/UStruct/UClass/UProperty/UEnum/UConst… (reader + exact inverse writer)
  src/bytecode.rs              Token tree, disassembler, assembler (tracks in-memory offsets: object refs are 8 bytes in memory, 4 on disk)
  src/context.rs               Multi-package object database: loads packages from search paths, resolves imports, class hierarchy, natives
  src/par.rs                   Thread helpers (parallel map, dependency scheduler) over std::thread::scope; `-j` / USCRIPT_JOBS
  src/toolchain.rs             Whole-package operations shared by the CLI and `upk serve`: decompile_package (all classes, planned
                               dependson), compile_package_dir (+ --conform setup), script_packages / build_order of a cooked dir
  src/compiler/                Port of FScriptCompiler (UnScrCom.cpp) + SavePackage ordering
    preprocess.rs              Header pre-scan (class/extends/dependson), cpptext/defaultproperties split, comment stripping
    lexer.rs                   FScriptCompiler::GetToken (incl. #linenumber)
    driver.rs                  Multi-pass driver: parse order, class tree, passes, CompileRequest
    engine.rs                  Compiler state: nests, name table (intern_name / intern_name_exact), emit helpers
    decl.rs                    Class/struct/enum/var/function/state declarations and specifiers
    expr.rs  field.rs  stmt.rs Expression / field-expression / statement compilation
    scope.rs                   FindField over the compiled package + loaded packages
    types.rs                   FPropertyBase equivalents, GConversions table
    link.rs                    UStruct::Link property layout (64-bit sizes, NeedCtorLink)
    defaults.rs  values.rs     defaultproperties: sub-object instancing, component maps, binary struct encoding
    model.rs                   Object graph produced by compilation
    exportsort.rs              SavePackage export order and NetIndex assignment
    output.rs                  Emitter: builds the package (names, imports, exports, load flags), --conform merging, summary
  src/decompile/
    mod.rs  decl.rs            Class header, declarations, enums, structs, states
    expr.rs  flow.rs           Expression printing (cast stripping, hints) and control-flow recovery (if/else/loops/switch/goto)
    defaults.rs                defaultproperties / Begin Object blocks
    plan.rs                    Minimal dependson() sets so the compiler parses in a valid order
  src/bin/uscript.rs           CLI (modules: dccmds, cccmds, dumpcmds, projcmds, …)

scripts/verify.sh              Full round-trip check against a cooked game directory (see Verification)
```

Reference material (not in this repo):

- UE3 10897 source: `~/dev/personal/UnrealEngine3-10897/Development/Src/` — `UnrealEd/Src/UnScrCom.cpp` (compiler), `Editor/Src/UnEdFact.cpp` (class-header scan), `Core/Src/UnCoreNative.cpp` (UEnum), `Core/Src/UnLinker*.cpp`/`UnObj.cpp` (SavePackage).
- The game binary with DWARF symbols is open in Binary Ninja (binja MCP) for places where Hi-Rez changed the engine.
- Game packages: `~/Games/Tempest/<version>/ChaosGame/CookedPCConsole/*.upk` (0.57 is the baseline; an 8.1 set lives in `~/Downloads/CookedPCConsole`).
- Reference UDK mod sources: `~/dev/personal/TgMod/Development/Src/{TmCore,TempestMp}`.

## Commands

```bash
cargo build --release                    # target/release/uscript  (or: just unrealscript-build)
cargo test --release                     # a handful of unit tests
cargo clippy --release --all-targets     # warnings are tolerated, errors are not
just unrealscript-check                  # test + clippy (part of `just check`)
just uscript <args>                      # run the CLI through the task runner
```

CLI (run `uscript` for the full list):

```
uscript info|names|imports|exports <pkg>              header tables
uscript disasm <pkg> <idx|path>                       token tree of one struct
uscript dump <pkg> [out.txt] [-I dir]... [--no-pos] [--hash|--hash-full]
                                                      normalised dump of every export, for diffing
uscript decompile <pkg> [-I dir]... [-o outdir] [-j N] [Class]...
                                                      writes <outdir>/<Pkg>/Classes/*.uc
uscript compile <srcdir/Pkg> -o out.upk [-I dir]... [-P dep]... [flags]
uscript project init <cooked-dir> <project-dir> [-j N]
uscript project build <project-dir> [-o outdir] [-j N] [Pkg]...
```

Parallelism (`-j N`, default all cores; `USCRIPT_JOBS` env overrides the default): `decompile` renders classes concurrently against the shared read-only context, `project init` scans package headers concurrently (`Package::load_tables`, no content decompression), and `project build` runs packages whose dependencies are built concurrently. A single `compile` is deliberately single-threaded: name interning and import creation order determine the bytes, so the compiler passes and the emitter must stay sequential. Output is identical at every `-j`; `-j 1` is the reference when in doubt.

Compile flags:

| Flag | Meaning |
|---|---|
| `-I dir` | package search path (first match wins; list rebuilt outputs before the game dir) |
| `-P dep` | dependency package to load; an empty list means the built-in defaults, so pass `-P None` for a package with no dependencies (Core) |
| `--cooked` | cooked output: drop deprecated / non-bool editoronly fields, cooker tag nulling, etc. |
| `--strip-source` | no TextBuffer/ScriptText exports |
| `--compress` | LZO chunk layout identical to the cooker's |
| `--conform <orig>` | patch mode: seed names/imports from `<orig>`, copy every export the compile does not produce (content, ShaderCache), conform NetIndex, component maps, tags, Line/TextPos, name flags, summary |
| `--allow-config-defaults` | accept defaultproperties values for config vars |
| `--server` | server field layout (untested) |

`project init` only picks packages that define top-level classes of their own and derives dependency order from import tables. It refuses to run if a package whose classes are imported is missing from the directory (e.g. GFxUI for TgClient), because a missing parent package would otherwise silently drop `extends`.

## Verification (the harness)

There are almost no unit tests. The test is the game:

```bash
scripts/verify.sh ~/Games/Tempest/0.57/ChaosGame/CookedPCConsole     # or: just unrealscript-verify <dir>
```

It runs `project init` + `project build` into `target/verify/` and prints `<Pkg>: N diff lines` per package from `uscript dump --hash` of original vs rebuilt. **Every cooked game package must report 0.** Packages that are not cooked (the user's UDK-built mods TmCore/TempestMp) differ in a few functions because UDK compiled them against UDK's Engine (e.g. `Canvas.DrawText` has one more optional parameter in the game); that is expected and not a regression.

To investigate a mismatch:

```bash
U=target/release/uscript; C=<cooked dir>; W=target/verify
diff $W/dumps/TgGame.orig $W/dumps/TgGame.ours | head          # which exports, which tokens
$U dump $C/TgGame.upk a.txt -I $C --no-pos --hash-full          # full payload hex when the token dump matches but bytes differ
$U info|names|imports|exports $C/TgGame.upk                     # header tables (strip ` @offset` with sed before diffing)
```

Fast single-package loop while iterating: `uscript decompile` → edit nothing → `uscript compile ... --cooked --strip-source --allow-config-defaults --compress --conform <orig>` → `uscript dump --hash` both and diff. Always run the full verify on 0.57 before finishing, and on the 8.1 set when it is available; the two versions exercise different code.

## How to add or fix a rule

1. Reproduce with the dump diff. Identify the export and token that differs.
2. Find what the engine does: read the 10897 source first. If the game clearly deviates (it does in places: see list below), confirm in the binary with binja rather than guessing.
3. Port the engine's logic literally, in the module that mirrors the engine function (`CompileExpr` → `compiler/expr.rs`, `SavePackage` sorting → `exportsort.rs`, …). Comment the engine function name next to the port. Do not add heuristics that merely make the current diff disappear: the `explicit trailing _MAX` exception and the `bool()`-stripping were both such shortcuts and both broke on 8.1.
4. The decompiler must print source that recompiles to the same bytes. If a cast or conversion is automatic in the engine's `GConversions` table (AUTOCONVERT), the decompiler strips it; otherwise it must be printed.
5. Run `scripts/verify.sh` on every available game version. 0 diff lines on all cooked packages, or explain exactly why not.
6. Record any new game-specific rule in the list below.

## Game-compiler and engine rules already ported (keep them)

Hi-Rez additions and deviations from 10897:

- State-scoped `var` (EX_StateVariable). `safeobject<Class>` property type: `cpf::SAFE_OBJECT` (bit 46) plus Component|ExportObject for component classes, tag type `SafeObjectProperty`, passes uncast to typed object parameters, cast 100 to plain `Object`.
- byte locals accepted for enum parameters; int → enum-byte auto conversion (the `Enum==NULL` guard is absent); `exec` is not part of the override-specifier check; `client` does not imply `simulated`; private functions of another package only warn.
- Delegate-property tokens carry no TokenFunction (context size byte CPT_Delegate=7). `Map` properties get NEED_CTOR_LINK unless native.
- DominantDirectionalLightComponent / DominantSpotLightComponent non-CDO templates have a 4-byte native prefix.
- Explicit casts to interfaces stay DynamicCast; InterfaceCast only comes from automatic conversion (operator operands, array AddItem/RemoveItem/InsertItem/Find items, arguments).

Engine rules that matter for identical output:

- Header pre-scan uses `appStrfind` semantics: a keyword only matches when not preceded by a letter or digit (`TgInvListener_CaspianExtendStackLifetime` contains `ExtendS`).
- No conversion to bool is automatic: `bool(x)` in a condition is always explicit.
- Enum `_MAX` is exactly `GenerateEnumPrefix() + "_MAX"` (common tag prefix up to the last `_`, else the enum name); any other trailing `_MAX` tag is an ordinary tag (`TG_PAWN_PROPERTIES` ends `TGPIDX_MAX, TG_PAWN_PROPERTIES_MAX` because two misspelt `TGIPDX_` tags break the prefix).
- Cooked output drops deprecated properties and non-bool editoronly properties (fields and values); editoronly bools stay. Binary (immutable) struct members serialise in PropertyLink order, enum values as names, `ShouldSerializeValue` skips native always, transient unless serialising defaults, deprecated when cooked.
- `#linenumber N` is handled in the lexer; the decompiler emits it before every `assert`.
- `ignores` stubs bind through the class chain only; a stub whose super is in a state is a bodiless prototype.
- `Class.Enum` / `Class.Struct` through a component class adds CPF_Component|ExportObject.
- `Components.Remove(X)` records RemovedComponents; component overrides and inherited copies instance the template's ctor-linked sub-objects; unreferenced sub-objects are not saved; an explicit `Begin Object Class=X Name=N` for an already-instanced copy refines it.
- Ternary branches get the required type as hint; the decompiler keeps explicit `byte(cond ? a : b)` (`hint_sensitive`).
- Load flags follow NeedsLoadForClient/Server (PrimitiveComponent collision/AlwaysLoad, AnimNode bEditorOnly), propagated to templates; CDOs always load.
- The decompiler emits stand-ins for things the cooker removed so the chain shape is reproduced: `var editoronly int EditorOnlyStripped;`, `var editoronly string EditorOnlyStripped;` in structs that lost a ctor-link member, `EditorOnlyStripped_<Type>` locals for nulled bytecode references.

## Asset layer (`tempest-assets`, CLI `upk`)

```bash
upk info|ls <pkg> [filter]                    tables (lazy: only the header chunk is expanded)
upk dump <pkg> [<idx|path>] [-I cooked-dir]   typed properties of one export (or all)
upk extract <pkg> <idx|path> <out>            raw payload of one export
upk verify <dir|pkg>... [-I dir] [-j N] [--classes] [--compressed] [-v]   (just upk-verify <dir>)
upk imagediff <pkg>                           where a rebuilt image / recompressed file first differs
upk texture|swf|sound|mesh ...                asset commands (see the sections below)
upk prop get|set, obj rename|delete|copy, deps, mod-dir   editing (see "Editing")
```

How an export is read (`object.rs`): class resolved by name in the `ClassDb` (content packages store every name lowercased; script packages keep case, so all lookups are case-insensitive); `UObject::Serialize` prefix via `tempest_unrealscript::objects::read_object_header` (`FStateFrame` when `RF_HasStack`, `UComponent::PreSerialize` for non-CDO components with `TemplateName` only when the object or an outer is a CDO, `NetIndex`); tagged properties decoded against the class (`value.rs`); everything after is the class's native payload, kept verbatim until a serializer for that class exists. `UDominant{Directional,Spot}LightComponent` write `TArray<WORD> DominantLightShadowMap` *before* `Super::Serialize` (empty on client packages, populated on server test maps).

Values: the decoder is self-checking — every tag is re-encoded and compared with the original bytes; anything that differs or cannot be typed (unknown struct, map property) stays `Value::Raw`, so the object round-trips regardless and the verify report counts it as "partial". Binary (`STRUCT_Immutable`, or `ImmutableWhenCooked` on cooked packages) struct members follow `PropertyLink` order and `ShouldSerializeValue` (native never, transient only for CDOs, deprecated and non-bool editor-only never on cooked data). Byte enums are `FName`s when the tag or the property definition names an enum.

Verification: `just upk-verify <cooked-dir>` parses and rewrites every export of every package, rebuilds each package image and compares. Required state on the 8.1 client set (993 packages), the 8.2 `CookedPCServer` set (740) and 0.57 (390): 0 mismatches, 0 unparsed headers, 0 raw values, every image identical. The "fully understood" percentage (no native tail left) is the progress metric for the asset serializers. Recompression is informational only: the cooker uses LZO Professional (`appCompressMemoryLZO` → `lzopro_lzo1x_1_14_compress`, `_1_08` with `COMPRESS_BiasSpeed`, `_99` with `COMPRESS_BiasMemory`), whose stream our LZO1X encoder cannot reproduce byte-for-byte; chunk tables do match.

### Textures

`upk texture info|export|export-all|import` (`texture_ops.rs`). How the game stores them (all confirmed against the 8.2 server binary):

- `UTexture2D::Serialize` = `SourceArt` bulk, `Mips` (count, then per mip: bulk header `flags, count, size on disk, offset`, inline payload when neither separate nor unused, then `SizeX`, `SizeY`), `TextureFileCacheGuid`, `CachedPVRTCMips`, `CachedFlashMipsMaxResolution`, `CachedATITCMips`, `CachedFlashMips` bulk, `CachedETCMips`; `ULightMapTexture2D` appends `LightmapFlags` (u32). Cooked inline bulk data has `offset == -1` (the cooker skips the write-back; the loader substitutes `Tell()`).
- Streamed mips carry flags `0x111` (separate | merged-file | LZO) with zero size/offset: `TextureFileCacheName` is the placeholder `Automated`, and `MergedFileIndexCache.bin` maps `MD5(upper-case engine path)` + mip index → `(size, offset, size on disk, file GUID)` and file GUID → `.tfc` name (`FMergedFileIndexCache`, `UObject::GetGuidFromPathName`). The engine path of an object copied into a seek-free package is its forced-export path (outermost `Package` export), not `<File>.<path>`. The `*PATCH.tfc` files are ordinary caches that the index points into. Payloads in the caches are plain LZO chunks (not XOR-obscured); the top mips of many textures are `unused` (stripped by LOD).
- `import` writes every mip inline (uncompressed, like the cooker's inline mips) so a modded package needs no cache or index change, sets `NeverStream`, `MipTailBaseIdx`, sizes, `Format`, drops `FirstResourceMemMip` and the export's summary pre-allocation entry (console RHIs only). Mip policy follows the texture (`--mips`/`--no-mips` override). **Not yet confirmed in the running game** — the first in-game test of an imported texture is still to do.

### UI movies, sounds, fonts

- `SwfMovie` (`GFxRawData`) has no native payload: the compiled movie is the script property `RawData` (`array<byte>`, a `GFX`/`CFX` file). `upk swf export|import` reads/replaces it (import accepts GFx or plain SWF).
- `AkBank` / `AkEvent` (Wwise) are property-only objects; the audio lives in the `.pck`/`.bnk` Wwise files beside the packages, outside this library's scope.
- `USoundNodeWave::Serialize` at version 893 is nine bulk members in order: RawData, CompressedPCData, Xbox360, PS3, WiiU, IPhone, Flash, two more platforms at +0x1a0/+0x308 (names guessed, the DWARF has none). PC audio is Ogg Vorbis stored raw inline. `upk sound export|import` extracts/replaces it and import updates `Duration`, `NumChannels`, `SampleRate`, `RawPCMDataSize` from the Ogg headers. Not confirmed in game yet.
- `UFont`/`UMultiFont`: `TMap<WORD,WORD> CharRemap` after the properties. `USoundCue`: `TMap<USoundNode*, FSoundNodeEditorData>` (empty when cooked). `UObjectRedirector`: one object index. `UGuidCache`: `TMap<FName, FGuid>`. `UShadowMap1D`: `TArray<float>` samples + `LightGuid` (4-byte samples in this build, not the 2-byte quantised struct of the source tree).

### Meshes and physics

`upk mesh info|export` (`mesh_ops.rs`). `UStaticMesh::Serialize` / `USkeletalMesh::Serialize` in the 8.2 binary match the 10897 source line for line; `mesh.rs` ports them for file version 893 only (every version branch resolved). Facts that matter:

- Server-cooked packages (`CookedPCServer`) omit the GPU-only buffers (`FStaticMeshVertexBuffer`, `FColorVertexBuffer`, skeletal GPU skin and colour buffers: `GCookingTarget == PLATFORM_WindowsServer`). Nothing in the package says which cooker ran, so `parse_native` tries the client layout, then the server one (`mesh::Layout { server }`), keeping whichever round-trips.
- Fixed-size element arrays stored with `TArray::BulkSerialize` (element size + count + raw) are kept opaque as `BulkArray`: kDOP nodes/triangles, vertex and index buffers, PhysX convex streams. `LODInfo` elements serialise nothing (count only).
- Cooked skeletal meshes have empty rigid/soft chunk vertex arrays; geometry comes from the GPU skin buffer (`TangentX, TangentZ, InfluenceBones[4], InfluenceWeights[4], Position, UVs`). `bUsePackedPosition` is set on PC meshes too but the cooker only packs for consoles: the stride decides (12-byte float position vs 4-byte `FPackedPosition`, X:11/Y:11/Z:10 low bits first, scaled by `MeshExtension` + `MeshOrigin`). The skeletal colour buffer is present only when the mesh's `bHasVertexColors` property is set.
- Export converts Unreal (Z-up, left-handed, cm) to glTF (Y-up, right-handed, m): `(x, z, y) / 100`, winding flipped. FBX (`mesh_fbx.rs`, binary 7.4 written by `fbx.rs`) uses the same axis swap but keeps centimetres (`UnitScaleFactor` 1): one `Mesh` model, normals and UVs per polygon vertex, materials per polygon, `Video.Content`-embedded PNGs connected to `DiffuseColor` / `NormalMap` / `EmissiveColor`, the skeleton as a `Null` root of `LimbNode` models (`Lcl Rotation` = Euler XYZ degrees of the local bind transform), a `Skin` deformer with one `Cluster` per weighted bone (`Transform` = inverse bind, `TransformLink` = global bind) and a `BindPose`. Blender imports it with the armature, 88 vertex groups and packed textures, and its re-export comes back through the importer with identical positions, normals and weights. Skeletal LOD > 0 sections go through `LODInfo[n].LODMaterialMap`; bone indices are 16-bit.

Importing (`upk mesh import <pkg> <obj> <in.glb|gltf|fbx|obj> -o out [--lod N]`, `mesh.import` in the Studio): the file is read in the exporter's convention (metres for glTF/OBJ, `UnitScaleFactor`-scaled centimetres for FBX, Y up, flipped winding; `(x, y, z) → (x·100, z·100, y·100)`; FBX polygons are fan-triangulated, every layer mapping is accepted, the model's `Lcl Translation/Rotation/Scaling` + `PreRotation` chain is applied, skin weights come from the `Cluster` deformers with bones matched by model name, ASCII FBX is refused), sections match material slots by name (object path or leaf) else by order. Static meshes: 16-bit index buffer (≤ 65535 vertices), `FPackedNormal` tangents from the UVs, half or float UVs as the LOD had, bounds (tight sphere), kDOP rebuilt with the engine's compact build (same node counts as the cooker: 68 triangles → 16 nodes); the `RB_BodySetup` (PhysX cooked convexes) is untouched. Skeletal meshes: the file's skin joints must be named like `RefSkeleton` bones (unknown joints are dropped from the weights), vertices are chunked so a chunk maps ≤ 75 bones, all vertices are "soft" (counts only, the cooked arrays stay empty), `ActiveBoneIndices` = used bones, `RequiredBones` = used + ancestors, bounds follow `UnMeshEd.cpp` (box doubled, bottom tucked 10%, radius = extent length) and reproduce the originals exactly on a round trip. Round-tripping an exported mesh through the importer reproduces counts, bounds and kDOP sizes; whether the game renders imported meshes has not been tested yet.

### Animations, lighting, BSP, levels, materials

All ported from the 10897 source and checked against the 8.2 binary (`level.rs`, `material.rs`, `actors.rs`); every version branch resolved for 893. Rules that were not obvious:

- `TTransArray<T>` (`ULevelBase::Actors`, `UModel::Surfs`) serialises its **owner object index before the array**; `UPolys` writes `DbNum, DbMax, Owner` by hand.
- `FLightMap*` is polymorphic: a `DWORD` type (0 none, 1 `FLightMap1D`, 2 `FLightMap2D`) then the payload. `NUM_STORED_LIGHTMAP_COEF` is 3 (two directional + one simple).
- `FStaticMeshComponentLODInfo` at 893: shadow maps, shadow vertex buffers, light map, `bLoadVertexColorData` byte + `FColorVertexBuffer`, `PaintedVertices`. `tgstaticmeshcomponent` and the other Hi-Rez subclasses add nothing.
- Materials: Hi-Rez's build has **three** material quality levels (quality mask 7), not the two of the source; `FMaterialResource` at 893 carries no uniform expressions (they live in the shader cache). `UMaterialInstance` has a payload only when `bHasStaticPermutationResource` is set.
- `AInstancedFoliageActor` (Hi-Rez) appends a `StreamGroups` array when `StreamingSerializationVersion == 1`; always empty in shipped maps, kept raw. `AHavokNavMeshActor` / `UHavokNavMeshGrid` are one bulk-data member each (Havok AI stream, opaque).
- `ULevel::Serialize` includes the APEX cached-data block (`DWORD` size + bytes), cached PhysX stores, the precomputed light volume, visibility buckets and the volume distance field, all as in the source.

### Editing (`edit.rs`, `visit.rs`)

`Editor` parses exports on demand and writes back only touched ones, saving with the original compression/obscure settings.

- `obj delete`: the export (and its sub-objects) become the cooker's zero-size, class-less `none` placeholders — no index shifting — and every reference to them in tables and payloads is cleared (`visit.rs` reaches every `PackageIndex`/`FName` in every `Native`; keep it in sync when adding serializers).
- `obj copy`: copies the dependency closure from the source package (sub-objects, materials, textures, package groups), reusing destination objects that already exist at the same path, turning script objects (classes, functions) into imports, interning names, and setting `NetIndex` to -1 on copies (not replicated; the cooker's per-package NetIndex numbering is not reproduced). `--shallow` clears references to objects the destination lacks instead.
- `prop set`: paths are `Name`, `Name[i]` (static array slot or dynamic element), `Outer.Member`, `Array[i].Member`; values parse by the property kind (ints, floats, `True`/`False`, enum tags, `'Name'`, `"text"`, `Class'Path'` or `None` for objects, which must already exist in the package). Missing properties are created from the class reflection.
- `mod-dir`: writes `manifest.toml` + `files/<pkg>` for `tempest mod pack` / `just mod-pack` (signing stays in the .NET CLI).

glTF export (`mesh_ops.rs`, `material_ops.rs`): materials resolve `TextureParameterValues` up the `Parent` chain, then the base material's `MaterialExpressionTextureSample*` sub-objects; slots are classified by Hi-Rez's parameter names (`diffuse`, `normal`, `specular`, `emissive`, `rgbmask`), other textures land in `extras.textures`. Textures are embedded as PNG (largest available mip, cross-package references opened from the cooked dir; an import whose root is not a package file, e.g. `pc_mastermaterial.…` cooked into a `*_SF` package, goes through `PackageCache`'s path lookup: a lazy scan of every package's export table on the CLI, the Studio's search index in `serve`; what still cannot be found is reported as a warning instead of silently dropped). Cube maps and other non-`Texture2D` classes are skipped. A cooked `Material` without `MaterialExpressionTextureSample*` sub-objects (map packages strip them) falls back to the compiled resource's `UniformExpressionTextures`, classified by texture name; a mesh whose sections have no material (map props: cooked maps set them on the components) takes the `Materials[]` of the first `MeshComponent` using it. An emissive map identical to the base colour is dropped. Skeletal meshes get a skin: one node per bone with its local matrix (`FQuat`+position, column-major, converted with the y/z swap), inverse bind matrices from the bind pose, `JOINTS_0`/`WEIGHTS_0` from the GPU skin influences (bone-map resolved per chunk). FBX export and import live in `mesh_fbx.rs`.

Deliberately still raw: `ShaderCache` (compiled shader bytecode, `FShaderCache::Load`; 1.3 GB on 8.1, nothing to edit), `TgPersistentCookerData` (one cooker bookkeeping export, no symbols in the binary), and in the dev server / 0.57 sets only: terrain, decal components with static receivers, landscape components. Coverage: 8.1 client 99.93 % of exports fully typed, 8.2 server 100.00 %, 0.57 99.96 %.

Container rules learned here: offsets in a compressed package are in the coordinates of its uncompressed form (the summary is laid out without the chunk table, which `compress_image` adds afterwards); zero-size placeholder exports have `SerialOffset` 0; `probe_layout` falls back to `Class` exports for script packages with no property exports (server `BattleGame`).

### Particle systems (`particle.rs`)

Cooked `ParticleSystem` → `ParticleSpriteEmitter.LODLevels[]` → `ParticleLODLevel { RequiredModule, SpawnModule, TypeDataModule, Modules[] }`; every `Distribution*` object is gone and each module property of type `RawDistributionFloat/Vector` is a tagged struct `{ Distribution = None, Op, LookupTableNumElements, LookupTableChunkSize, LookupTable, LookupTableTimeScale, LookupTableStartTime }` holding what the curve evaluated to: `LookupTable[0..2]` are the min and max, then entries of `ChunkSize` floats (`NumElements` values of `ChunkSize / NumElements` components; two values when `Op` is random (1) or extreme (2)), entry `i` at time `StartTime + i / TimeScale`, lerped (`FRawDistribution::GetEntry`). Objects only serialise what differs from the class default object, and the cooked CDOs (`Engine.upk`, `Default__ParticleModuleSpawn` …) carry complete baked tables (e.g. `RateScale` = 1, `Op` = 1), so an absent table or member means the CDO's: `serve.rs` merges them (`cdo_distributions`, child class first) into the JSON the Studio simulates. `particle::set_distribution` writes a table (and header fields) into the instance; `set_member` matches struct members case-insensitively because content packages lower-case their names (a case-sensitive match used to append a duplicate member). The Studio's simulation and curve editor are `Tempest.Studio/src/lib/studio/particles.ts`, `ParticlePreview.svelte`, `CurveEditor.svelte`; mesh emitters reuse `mesh.preview`.

### Wwise audio (`wwise/`, `audio.rs`)

Paladins plays its sounds through Wwise: the `AkEvent`/`AkBank` exports are name stubs, the audio sits in 584 `WWB_*.pck` + `Init.pck` file packages (`AKPK` v1) holding soundbanks (`BKHD` version 120 = Wwise 2019) with embedded media and streamed media, all Wwise Vorbis (43 489 files, one container layout: 66-byte `fmt`, no `vorb` chunk, 2-byte packet headers, external codebooks, mod packets; 196 carry a seek table). Ids are FNV-1 of lower-case names (`wwb_pc_khan` → bank id and `WWB_PC_Khan.pck`; event names from the AkEvent exports). `upk wwise ls|event|export|import|convert|rewrite` is the CLI; the Studio methods are `wwise.{packages,open,preview,export,import,replacePcm,forObject}` and `sound.replacePcm`.

Converting: `vorbis::wem_to_ogg` is ww2ogg's algorithm (codebooks rebuilt from the library, floors/residues/mappings/modes expanded, audio packets given back their type bit and window flags; pages get real granule positions, which ww2ogg leaves at 0). `vorbis::ogg_to_wem` is the inverse and only accepts streams whose codebooks are all in the library, i.e. encoded by the aoTuV flavour of libvorbis (what `audio::encode_ogg` is). The header fields were learned from the game's files: `uHashCodebook` = FNV-1 of the setup packet (verified on all six setups the game uses), `dwDecodeAllocSize`/`dwDecodeX64AllocSize` are per setup (copied when the hash matches a known one, else the largest pair), the channel config is `mask << 12 | 1 << 8 | channels`. Encoding mono at quality 0.7 or 0.8 reproduces the game's own mono setups byte for byte (hashes 46ffffe1 / 5921d930); the stereo setups are not reproduced at any quality (a different encoder configuration), so stereo replacements get their own hash. Whether the game plays replaced media has not been tested in game yet. Every pck re-serialises byte-identically (`upk wwise rewrite`); a bank's `DIDX`/`DATA` are rebuilt 16-byte aligned on replace and each sound/music-track source of the media gets the new in-memory size. Container children are found by scanning for the parent id in candidate objects of the kinds that node may contain (`NodeBaseParams` is not parsed).

### Studio backend (`serve.rs`, `json.rs`)

`upk serve` is a JSON-lines RPC server over stdin/stdout for `Tempest.Studio`: one request per line (`{"id", "method", "params"}`), one response per line (`{"id", "ok", "result" | "error", "ms"}`), plus id-less event lines (`{"event": "search.progress", ...}`). It keeps the `ClassDb` (leaked to `'static` on `workspace.open`, so editors can borrow it), the `TextureSource`, one `Editor` per opened package (keyed by canonical path) and a per-package search index in memory (built on a background thread from `workspace.open`, ≈2 s for a full cooked folder, joined by `index_all` before the first search, batch plan or textured mesh/material preview). Methods: `workspace.{open,packages,search,summary}`, `package.{open,tables,close,dirty,save,rewrite,verify}`, `object.{get,text,set,rename,delete,copy,deps,extract}`, `texture.{info,preview,export,exportAll,import}`, `mesh.{info,preview,export}`, `media.{preview,export,import}`, `mod.dir`. `script.{source,compile,export}` drive `tempest_unrealscript::toolchain`: `source` decompiles the package once (cached) and returns the class owning the export; `compile` writes the cached sources with one class replaced to a temp dir, compiles them conformed to the file on disk (`CompileOptions::for_replacement`, dependencies from `build_order` of the workspace) and swaps the result into the open `Editor` (`replace_package`, dirty until saved); errors come back as `compile failed: {"class","line","detail","message"}`. `json.rs` renders tables, parsed objects and property values (`{"t": kind, "v": ...}`; references as `{ref, path, class}`); previews travel as base64. Saving in place drops and reopens the editor, because the original is memory-mapped. The TypeScript mirror of the shapes is `Tempest.Studio/electron/src/ipc/contract.ts` (`UpkMethods`): keep both in sync.

## Gotchas

- `uscript decompile` writes into `<outdir>/<Pkg>/Classes`; a stray positional argument is a class filter, not an output dir, and output then lands in the current directory.
- `--conform` seeds the name table with the reference's exact spellings before `None`; never reorder that.
- `-P` with no entries means the default dependency list, hence `-P None` for Core.
- `dump --hash` compares payload hashes; `--hash-full` prints full hex. Use `--no-pos` to ignore Line/TextPos.
- Nothing has been loaded in the game yet from recompiled packages; byte-identical output is the proxy for correctness.

### MCTS marshals (`crates/tempest-assets/src/mcts/`)

`assembly.dat` (CookedPCConsole), `Localization/<EXT>/Lang_<EXT>.dat` and `WordFilter.dat` are Hi-Rez `CMarshal` streams XOR'd with 0x2A (no compression, no hash): `u8 flags`, `u32 function` (FNV-1 of the name; `GET_DATA_ASSEMBLY`, 0 for localization), then a row (`u16` entry count, 0xFFFF → `u32`) of entries whose `u16` header is kind (bits 15..12) and parameter (bits 5..0): 1..4 batches of 8/16/32/64-bit numbers (`param` tokens then values), 5/9 legacy strings, 6 row set, 7 GUID, 8 blob, 10 string with the encoding in the parameter (the files use 4, UTF-16). Fields are tokens into the game's `MFIELD_TOKEN` enum; `mod.rs` holds the reader, the writer (which follows `CMarshalRow::StoreEntries`: per row the numbers are batched into four width caches flushed when 32 are gathered and at the row's end in width order, the width from the field's `MFIELD_TYPES` class and, for ints/dwords/floats and words, from the value; 64-bit for double/qword/netid/datetime) and `Tokens` (the field and function tables: extracted at runtime from the game's `Binaries/Win64/MctsInterface.dll` next to the cooked folder, gzip blobs with `VERSION` / `HELLO`, else the built-in copies from 8.1 in `data/mcts/`). `doc.rs` is an editable document (rows addressed by a path of `(field, index)` pairs, undo journal, paged/filtered/sorted queries), `workspace.rs` the assembly + languages of a cooked folder with the JSON shapes `upk serve` exposes as `mcts.*` (open, table, rows, row, set, setHeader, insert, delete, undo, save, discard, find, messages, setMessage, validValues). Correctness: `upk mcts verify <dir>` must rewrite every `.dat` byte-identically (it does for 8.1 and 8.2). Verified against `CMarshalRow::LoadEntries` / `StoreEntries` and `CPackage::ReadFromFile` in `libMctsInterface.so`. Unknown: whether the client compares `GAME_VERSION_RAW` (four `u16`: 8.1.5839.0) with its own build; the editor keeps it and lets it be edited. `behavior_trees.dat` is not a marshal.

### Serve performance notes

`ClassDb::load_files` parses the script packages in parallel (`Context::load_files_parallel`, `par::map`; 0.75 s for 8.1's 16 packages) and `list_packages` reads the 993 summaries in parallel. The search index is persisted compressed under the OS cache folder (`<cache>/tempest-studio/index/<hash>.idx`, entries reused while a package's size and mtime match). `upk serve` answers framed after `{"method":"protocol","params":{"frames":true}}`: header line `{id, ok, ms, len, bin:[…]}` + result JSON + raw payloads (`BinSink`, `{"$bin": n}` markers); without the handshake payloads are base64 as before (scripted sessions). `package.open`/`package.tables` return the export table as columns (`json::tables_compact`).

### Movies (`swf.rs`)

GFx/SWF container (`GFX`/`FWS` plain, `CFX`/`CWS` zlib; LZMA refused), header rect/rate/count kept as bytes, tags with their original length form so an untouched movie rewrites byte-identically (every movie of the 8.1 UI packages does). Parsed for the Studio: `DefineEditText` (flag bytes are a big-endian bit field), `DoInitAction`/`DoAction` strings (constant pools, pushes) and a disassembly, `DefineExternalImage2` (u32 id, format, size, export name, file name), `ExportAssets`, `ImportAssets2`, sprites. Edits re-serialise only the touched tag; script string edits are limited to top-level actions (relative branches are unaffected, `DefineFunction` bodies are not touched).

### Content creation (`mcts/create.rs`)

Devices (abilities, weapons, cards and talents are all `DATA_SET_DEVICES` + `DATA_SET_ITEMS` rows with one id), items and champions are cloned from a template row: ids past the current maxima, nested `DEVICE_MODE_ID`s reallocated, messages created in every language. `scaffold` writes the mod folder (see the root AGENTS.md). A champion's visibility in the client is decided by loot table 475 "Champion Vendor" (`DATA_SET_LOOT_TABLES` / `DATA_SET_LOOT_TABLE_ITEMS`): the native `UIDataChampions` init (8.1 client `sub_1415b7d60`) walks that table's entries, skips inactive ones (`ACTIVE_FLAG`), makes a `UIDataChampion` per Character Class item (`m_nBotId` = the item's `REF_BOT_ID`) and fills `m_Champions` + `m_ChampionIdToData`; no server data is involved, which is why an offline client still lists every champion. `create_champion` therefore appends an entry for the new Character Class item (`add_vendor_entry`: clone of the template's entry, fresh `LOOT_TABLE_ITEM_ID`, next `SORT_ORDER`, active, without the template's own `*_MSG_ID` overrides); `mcts.vendor` reports the table (active/total, listed entries, champion bots with a Character Class item but no entry) and `mcts.vendorAdd {bot}` adds one for an existing bot. The in-match select movie has 60 button clips and the native binding loop is hard-coded to 60 (8.1 ships 60 active entries), so a 61st champion needs one deactivated. Playability also needs the private server to list the champion (`server_rows`).
