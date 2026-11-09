# UnrealScript: assets

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Asset layer (`tempest-assets`, CLI `upk`)

```bash
upk info|ls <pkg> [filter]                    tables (lazy: only the header chunk is expanded)
upk dump <pkg> [<idx|path>] [-I cooked-dir]   typed properties of one export (or all)
upk extract <pkg> <idx|path> <out>            raw payload of one export
upk compress|decompress <pkg> <out> [--obscure]   rewrite LZO-compressed (cooker chunks) / uncompressed
upk is-compressed <pkg>                       `true`/`false`: whether it is stored LZO-compressed (the launcher's check)
upk verify <dir|pkg>... [-I dir] [-j N] [--classes] [--compressed] [-v]   (just upk-verify <dir>)
upk imagediff <pkg>                           where a rebuilt image / recompressed file first differs
upk texture|swf|sound|mesh ...                asset commands (see topic references in ../AGENTS.md)
upk prop get|set, obj rename|delete|copy, deps, mod-dir   editing (see editing.md)
upk live baseline|changes                     state of the movie packages for the hot reload mod (see hot-reload.md)
```

How an export is read (`object.rs`): class resolved by name in the `ClassDb` (content packages store
every name lowercased; script packages keep case, so all lookups are case-insensitive);
`UObject::Serialize` prefix via `tempest_unrealscript::objects::read_object_header` (`FStateFrame`
when `RF_HasStack`, `UComponent::PreSerialize` for non-CDO components with `TemplateName` only when
the object or an outer is a CDO, `NetIndex`); tagged properties decoded against the class
(`value.rs`); everything after is the class's native payload, kept verbatim until a serializer for
that class exists. `UDominant{Directional,Spot}LightComponent` write `TArray<WORD>
DominantLightShadowMap` *before* `Super::Serialize` (empty on client packages, populated on server
test maps).

Values: the decoder is self-checking — every tag is re-encoded and compared with the original bytes;
anything that differs or cannot be typed (unknown struct, map property) stays `Value::Raw`, so the
object round-trips regardless and the verify report counts it as "partial". Binary
(`STRUCT_Immutable`, or `ImmutableWhenCooked` on cooked packages) struct members follow
`PropertyLink` order and `ShouldSerializeValue` (native never, transient only for CDOs, deprecated
and non-bool editor-only never on cooked data). Byte enums are `FName`s when the tag or the property
definition names an enum.

Verification: `just upk-verify <cooked-dir>` parses and rewrites every export of every package,
rebuilds each package image and compares. Required state on the 8.1 client set (993 packages), the
8.2 `CookedPCServer` set (740) and 0.57 (390): 0 mismatches, 0 unparsed headers, 0 raw values, every
image identical. The "fully understood" percentage (no native tail left) is the progress metric for
the asset serializers. Recompression is informational only: the cooker uses LZO Professional
(`appCompressMemoryLZO` → `lzopro_lzo1x_1_14_compress`, `_1_08` with `COMPRESS_BiasSpeed`, `_99`
with `COMPRESS_BiasMemory`), whose stream our LZO1X encoder cannot reproduce byte-for-byte; chunk
tables do match.

### Textures

`upk texture info|export|export-all|import` (`texture_ops.rs`). How the game stores them (all
confirmed against the 8.2 server binary):

- `UTexture2D::Serialize` = `SourceArt` bulk, `Mips` (count, then per mip: bulk header `flags,
  count, size on disk, offset`, inline payload when neither separate nor unused, then `SizeX`,
  `SizeY`), `TextureFileCacheGuid`, `CachedPVRTCMips`, `CachedFlashMipsMaxResolution`,
  `CachedATITCMips`, `CachedFlashMips` bulk, `CachedETCMips`; `ULightMapTexture2D` appends
  `LightmapFlags` (u32). Cooked inline bulk data has `offset == -1` (the cooker skips the
  write-back; the loader substitutes `Tell()`).
- Streamed mips carry flags `0x111` (separate | merged-file | LZO) with zero size/offset:
  `TextureFileCacheName` is the placeholder `Automated`, and `MergedFileIndexCache.bin` maps
  `MD5(upper-case engine path)` + mip index → `(size, offset, size on disk, file GUID)` and file
  GUID → `.tfc` name (`FMergedFileIndexCache`, `UObject::GetGuidFromPathName`). The engine path of
  an object copied into a seek-free package is its forced-export path (outermost `Package` export),
  not `<File>.<path>`. The `*PATCH.tfc` files are ordinary caches that the index points into.
  Payloads in the caches are plain LZO chunks (not XOR-obscured); the top mips of many textures are
  `unused` (stripped by LOD).
- `import` writes every mip inline (uncompressed, like the cooker's inline mips) so a modded package
  needs no cache or index change, sets `NeverStream`, `MipTailBaseIdx`, sizes, `Format`, drops
  `FirstResourceMemMip` and the export's summary pre-allocation entry (console RHIs only). Mip
  policy follows the texture (`--mips`/`--no-mips` override). **Not yet confirmed in the running
  game** — the first in-game test of an imported texture is still to do.

### UI movies, sounds, fonts

- `SwfMovie` (`GFxRawData`) has no native payload: the compiled movie is the script property
  `RawData` (`array<byte>`, a `GFX`/`CFX` file). `upk swf export|import` reads/replaces it (import
  accepts GFx or plain SWF).
- `AkBank` / `AkEvent` (Wwise) are property-only objects; the audio lives in the `.pck`/`.bnk` Wwise
  files beside the packages, outside this library's scope.
- `USoundNodeWave::Serialize` at version 893 is nine bulk members in order: RawData,
  CompressedPCData, Xbox360, PS3, WiiU, IPhone, Flash, two more platforms at +0x1a0/+0x308 (names
  guessed, the DWARF has none). PC audio is Ogg Vorbis stored raw inline. `upk sound export|import`
  extracts/replaces it and import updates `Duration`, `NumChannels`, `SampleRate`, `RawPCMDataSize`
  from the Ogg headers. Not confirmed in game yet.
- `UFont`/`UMultiFont`: `TMap<WORD,WORD> CharRemap` after the properties. `USoundCue`:
  `TMap<USoundNode*, FSoundNodeEditorData>` (empty when cooked). `UObjectRedirector`: one object
  index. `UGuidCache`: `TMap<FName, FGuid>`. `UShadowMap1D`: `TArray<float>` samples + `LightGuid`
  (4-byte samples in this build, not the 2-byte quantised struct of the source tree).

### Animations, lighting, BSP, levels, materials

All ported from the 10897 source and checked against the 8.2 binary (`level.rs`, `material.rs`,
`actors.rs`); every version branch resolved for 893. Rules that were not obvious:

- `TTransArray<T>` (`ULevelBase::Actors`, `UModel::Surfs`) serialises its **owner object index
  before the array**; `UPolys` writes `DbNum, DbMax, Owner` by hand.
- `FLightMap*` is polymorphic: a `DWORD` type (0 none, 1 `FLightMap1D`, 2 `FLightMap2D`) then the
  payload. `NUM_STORED_LIGHTMAP_COEF` is 3 (two directional + one simple).
- `FStaticMeshComponentLODInfo` at 893: shadow maps, shadow vertex buffers, light map,
  `bLoadVertexColorData` byte + `FColorVertexBuffer`, `PaintedVertices`. `tgstaticmeshcomponent` and
  the other Hi-Rez subclasses add nothing.
- Materials: Hi-Rez's build has **three** material quality levels (quality mask 7), not the two of
  the source; `FMaterialResource` at 893 carries no uniform expressions (they live in the shader
  cache). `UMaterialInstance` has a payload only when `bHasStaticPermutationResource` is set.
- `AInstancedFoliageActor` (Hi-Rez) appends a `StreamGroups` array when
  `StreamingSerializationVersion == 1`; always empty in shipped maps, kept raw. `AHavokNavMeshActor`
  / `UHavokNavMeshGrid` are one bulk-data member each (Havok AI stream, opaque).
- `ULevel::Serialize` includes the APEX cached-data block (`DWORD` size + bytes), cached PhysX
  stores, the precomputed light volume, visibility buckets and the volume distance field, all as in
  the source.

### Particle systems (`particle.rs`)

Cooked `ParticleSystem` → `ParticleSpriteEmitter.LODLevels[]` → `ParticleLODLevel { RequiredModule,
SpawnModule, TypeDataModule, Modules[] }`; every `Distribution*` object is gone and each module
property of type `RawDistributionFloat/Vector` is a tagged struct `{ Distribution = None, Op,
LookupTableNumElements, LookupTableChunkSize, LookupTable, LookupTableTimeScale,
LookupTableStartTime }` holding what the curve evaluated to: `LookupTable[0..2]` are the min and
max, then entries of `ChunkSize` floats (`NumElements` values of `ChunkSize / NumElements`
components; two values when `Op` is random (1) or extreme (2)), entry `i` at time `StartTime + i /
TimeScale`, lerped (`FRawDistribution::GetEntry`). Objects only serialise what differs from the
class default object, and the cooked CDOs (`Engine.upk`, `Default__ParticleModuleSpawn` …) carry
complete baked tables (e.g. `RateScale` = 1, `Op` = 1), so an absent table or member means the
CDO's: `serve.rs` merges them (`cdo_distributions`, child class first) into the JSON the Studio
simulates. `particle::set_distribution` writes a table (and header fields) into the instance;
`set_member` matches struct members case-insensitively because content packages lower-case their
names (a case-sensitive match used to append a duplicate member). The Studio's simulation and curve
editor are `Tempest.Studio/src/lib/studio/particles.ts`, `ParticlePreview.svelte`,
`CurveEditor.svelte`; mesh emitters reuse `mesh.preview`.

### Wwise audio (`wwise/`, `audio.rs`)

Paladins plays its sounds through Wwise: the `AkEvent`/`AkBank` exports are name stubs, the audio
sits in 584 `WWB_*.pck` + `Init.pck` file packages (`AKPK` v1) holding soundbanks (`BKHD` version
120 = Wwise 2019) with embedded media and streamed media, all Wwise Vorbis (43 489 files, one
container layout: 66-byte `fmt`, no `vorb` chunk, 2-byte packet headers, external codebooks, mod
packets; 196 carry a seek table). Ids are FNV-1 of lower-case names (`wwb_pc_khan` → bank id and
`WWB_PC_Khan.pck`; event names from the AkEvent exports). `upk wwise
ls|event|export|import|convert|rewrite` is the CLI; the Studio methods are
`wwise.{packages,open,preview,export,import,replacePcm,forObject}` and `sound.replacePcm`.

Converting: `vorbis::wem_to_ogg` is ww2ogg's algorithm (codebooks rebuilt from the library,
floors/residues/mappings/modes expanded, audio packets given back their type bit and window flags;
pages get real granule positions, which ww2ogg leaves at 0). `vorbis::ogg_to_wem` is the inverse and
only accepts streams whose codebooks are all in the library, i.e. encoded by the aoTuV flavour of
libvorbis (what `audio::encode_ogg` is). The header fields were learned from the game's files:
`uHashCodebook` = FNV-1 of the setup packet (verified on all six setups the game uses),
`dwDecodeAllocSize`/`dwDecodeX64AllocSize` are per setup (copied when the hash matches a known one,
else the largest pair), the channel config is `mask << 12 | 1 << 8 | channels`. Encoding mono at
quality 0.7 or 0.8 reproduces the game's own mono setups byte for byte (hashes 46ffffe1 / 5921d930);
the stereo setups are not reproduced at any quality (a different encoder configuration), so stereo
replacements get their own hash. Whether the game plays replaced media has not been tested in game
yet. Every pck re-serialises byte-identically (`upk wwise rewrite`); a bank's `DIDX`/`DATA` are
rebuilt 16-byte aligned on replace and each sound/music-track source of the media gets the new
in-memory size. Container children are found by scanning for the parent id in candidate objects of
the kinds that node may contain (`NodeBaseParams` is not parsed).
