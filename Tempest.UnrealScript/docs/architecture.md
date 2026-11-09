# UnrealScript: architecture

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Overview

Rust toolchain for Paladins packages (Unreal Engine 3, build 10897, package file version 893): the
UnrealScript compiler/decompiler (`uscript`) and the asset layer (`upk`). The one rule that matters:
**output must be byte-identical to what the game's own compiler, cooker and `SavePackage` produce.**
Every change is judged by the round-trip harnesses in [verification](verification.md) and [asset
verification](assets.md), not by whether the result "looks right".

## What it is for

- Compile UnrealScript mods without UDK, against the game's own cooked packages.
- Decompile every script package of a game install into a UDK-style
  `Development/Src/<Pkg>/Classes/*.uc` tree, edit, and recompile everything.
- Read, extract, edit and insert the assets of any game package (`tempest-assets`, `upk`); the GUI
  editor builds on it.
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

crates/tempest-assets/         Typed object layer over any package (see assets.md)
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
  src/map.rs                   Maps as scenes: level actors + streamed sublevels, world matrices (FRotationMatrix, AActor::LocalToWorld,
                               FMatrix::Rotator), component drawables, brush geometry, set_transform, adding/removing actors (Studio `map.*`)
  src/kismet.rs                Kismet sequences as graphs (ops, connector DrawX/DrawY, links), link/unlink, adopt (new node from a copy), remove (Studio `kismet.*`)
  src/matinee.rs               Matinee timelines (SeqAct_Interp → InterpData → groups → tracks → key lists), key edits by property path, groups/tracks/connectors,
                               new Matinees (Studio `matinee.*`)
  src/matinee_curve.rs         FInterpCurve::Eval / AutoSetTangents / FClampFloatTangent, move-track key and world transforms (the
                               reference for the Studio's `matinee-math.ts`)
  src/matinee.rs               Matinee timelines (SeqAct_Interp → InterpData → groups → tracks → key lists), key edits by property path (Studio `matinee.*`)
  src/brush.rs                 Brush geometry as UnrealEd builds it: brush builders (Cube/Cylinder/Cone/Tetrahedron, hulls, polys), FPoly ops, the brush
                               BSP (`RebuildBrush`: bspBuild/SplitPolyList/bspAddNode/bspRefresh/FilterBound), KModelToHulls → HullFromPlanes →
                               GenerateHullData (FKConvexElem members; an incremental hull stands in for PhysX's)
  src/collision.rs             Collision of a map (volumes' BrushAggGeom, meshes' BodySetup / UseSimple*, flags through archetypes) and
                               `set_brush` (Brush Model + Polys + BrushAggGeom of a volume; Studio `map.collision|brush|setBrush`)
  src/lightbake/                Static light maps (Studio `light.*`, `upk light`): encode.rs (the 10897 light map encoding: gathered samples,
                               ConvertToLightSample, quantization, coverage mips + texel expansion, DXT1), scene.rs (lights, static mesh
                               components and world settings of a level with archetype/CDO defaults), bvh.rs (shadow/gather rays),
                               bake.rs (texel rasterization + Lightmass direct lighting, padding), write.rs (atlases, LightMapTexture2D, FLightMap2D)
  src/material.rs              Material / MaterialInstance resources (FMaterialResource, FStaticParameterSet; 3 quality levels)
  src/actors.rs                SeqAct_Interp, StaticLightCollectionActor, PrefabInstance, DecalComponent, InstancedFoliageActor, FluidSurface/SpeedTree
                               components, Pylon, MetaData, ApexGenericAsset, HavokNavMesh{Actor,Grid} (the actor's tagfile: `havok/`), SoundClass, Landscape components
  src/havok/                   Havok AI nav meshes (HavokNavMeshActor bulk data): tagfile.rs (Havok 2016 `TAG0` tagfiles: sections, types,
                               items, generic values, byte-identical write), navmesh.rs (typed hkaiNavMesh, cluster graph, annotation library,
                               validation, `upk havok dump`), tree.rs (hkcdStaticTree Codec3Axis6 decode/pack), build.rs (face AABB tree build:
                               calcFaceAabb, rebuildBranchSAH port, buildFromDynamicTree), edit.rs (delete faces, move vertices, write back),
                               view.rs (Studio buffers, regions), studio.rs (`map.navmesh` / `map.navmeshEdit`)
  src/material_ops.rs          PackageCache (cross-package import resolution) + material texture resolution (instance overrides → parent → expressions)
  src/visit.rs                 Visitor over every FName / object reference of an object (header, properties, every Native)
  src/edit.rs                  Editor: rename, delete (placeholder + reference clearing), cross-package copy (dependency closure), property get/set by path,
                               undo journal (begin_undo / snapshot / undo: per-export snapshots, table truncation, summary), save
  src/batch.rs                 Find & replace over a package (values + object names) and folder batch import (`<package>.<object>.<ext>`; a first part that is not a package file is a group, the whole name is then an object path found through a `PathLookup` in every package)
  src/delta.rs                 Mod deltas (`.tdelta`): canonical per-object hashes, create (mini package of changed objects + reference descriptors), apply (remap by path, two phases when classes are added), compare
  src/mcts/delta.rs            Row deltas for the marshal files (key column per table, add/set/delete by key)
  src/wwise/delta.rs           Deltas of Wwise `.pck` files (`kind: "pck"`): add/set/delete per table entry (`stream:<id>`, `bank:<id>`,
                               `external:<id>`, `@<lang>` when not 0) or per bank part (`bank:<id>/hirc:<obj>` kind byte + body,
                               `/media:<id>`, `/chunk:<TAG>`), MD5 `base` for drift, payloads in one stream; additions keep the tables
                               sorted and a new object before the object it preceded (children before parents); digest = hashes only
  src/basestore.rs             The Studio's record of a file before its first in-place save (digest under the OS cache folder), game-relative paths
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

- UE3 10897 source: `~/dev/personal/UnrealEngine3-10897/Development/Src/` —
  `UnrealEd/Src/UnScrCom.cpp` (compiler), `Editor/Src/UnEdFact.cpp` (class-header scan),
  `Core/Src/UnCoreNative.cpp` (UEnum), `Core/Src/UnLinker*.cpp`/`UnObj.cpp` (SavePackage).
- The game binary with DWARF symbols is open in Binary Ninja (binja MCP) for places where Hi-Rez
  changed the engine.
- Game packages: `~/Games/Tempest/<version>/ChaosGame/CookedPCConsole/*.upk` (0.57 is the baseline;
  an 8.1 set lives in `~/Downloads/CookedPCConsole`).
- Reference UDK mod sources: `~/dev/personal/TgMod/Development/Src/{TmCore,TempestMp}`.

- `Tempest.UnrealScript/` — Rust UnrealScript compiler/decompiler for the game's UPK packages (CLI
  `uscript`) and the typed asset layer over any package (`tempest-assets`, CLI `upk`: lazy mmap
  reader, reflection, typed properties, round-trip verifier), standalone Cargo workspace; see its
  `AGENTS.md` before touching it

## Tempest.UnrealScript commands

```bash
just unrealscript-build          # cargo build --release -> Tempest.UnrealScript/target/release/uscript
just unrealscript-check          # cargo test + clippy (part of `just check`)
just unrealscript-verify <dir>   # decompile + recompile every script package of a cooked game dir, diff against originals
just uscript <args>              # run the CLI, e.g. `just uscript project init <cooked-dir> <project-dir>`
just upk-verify <dir>            # asset layer: parse + rewrite every export of every package, coverage report
just upk <args>                  # package/asset CLI, e.g. `just upk dump <pkg> <idx|path>`
# `upk serve` is the JSON-lines RPC backend Tempest.Studio talks to (crates/tempest-assets/src/serve.rs)
```

Correctness is defined as byte-identical output versus the game's own packages; `scripts/verify.sh`
is the test suite. The [project guide](../AGENTS.md) routes to the layout, CLI, verification and
game-specific compiler rules that must be preserved.
