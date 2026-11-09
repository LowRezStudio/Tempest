# UnrealScript: meshes and animation

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Meshes and physics

`upk mesh info|export` (`mesh_ops.rs`). `UStaticMesh::Serialize` / `USkeletalMesh::Serialize` in the
8.2 binary match the 10897 source line for line; `mesh.rs` ports them for file version 893 only
(every version branch resolved). Facts that matter:

- Server-cooked packages (`CookedPCServer`) omit the GPU-only buffers (`FStaticMeshVertexBuffer`,
  `FColorVertexBuffer`, skeletal GPU skin and colour buffers: `GCookingTarget ==
  PLATFORM_WindowsServer`). Nothing in the package says which cooker ran, so `parse_native` tries
  the client layout, then the server one (`mesh::Layout { server }`), keeping whichever round-trips.
- Fixed-size element arrays stored with `TArray::BulkSerialize` (element size + count + raw) are
  kept opaque as `BulkArray`: kDOP nodes/triangles, vertex and index buffers, PhysX convex streams.
  `LODInfo` elements serialise nothing (count only).
- Cooked skeletal meshes have empty rigid/soft chunk vertex arrays; geometry comes from the GPU skin
  buffer (`TangentX, TangentZ, InfluenceBones[4], InfluenceWeights[4], Position, UVs`).
  `bUsePackedPosition` is set on PC meshes too but the cooker only packs for consoles: the stride
  decides (12-byte float position vs 4-byte `FPackedPosition`, X:11/Y:11/Z:10 low bits first, scaled
  by `MeshExtension` + `MeshOrigin`). The skeletal colour buffer is present only when the mesh's
  `bHasVertexColors` property is set.
- Export converts Unreal (Z-up, left-handed, cm) to glTF (Y-up, right-handed, m): `(x, z, y) / 100`,
  winding flipped. FBX (`mesh_fbx.rs`, binary 7.4 written by `fbx.rs`) uses the same axis swap but
  keeps centimetres (`UnitScaleFactor` 1): one `Mesh` model, normals and UVs per polygon vertex,
  materials per polygon, `Video.Content`-embedded PNGs connected to `DiffuseColor` / `NormalMap` /
  `EmissiveColor`, the skeleton as a `Null` root of `LimbNode` models (`Lcl Rotation` = Euler XYZ
  degrees of the local bind transform), a `Skin` deformer with one `Cluster` per weighted bone
  (`Transform` = inverse bind, `TransformLink` = global bind) and a `BindPose`. Blender imports it
  with the armature, 88 vertex groups and packed textures, and its re-export comes back through the
  importer with identical positions, normals and weights. Skeletal LOD > 0 sections go through
  `LODInfo[n].LODMaterialMap`; bone indices are 16-bit.

Importing (`upk mesh import <pkg> <obj> <in.glb|gltf|fbx|obj> -o out [--lod N]`, `mesh.import` in
the Studio): the file is read in the exporter's convention (metres for glTF/OBJ,
`UnitScaleFactor`-scaled centimetres for FBX, Y up, flipped winding; `(x, y, z) → (x·100, z·100,
y·100)`; FBX polygons are fan-triangulated, every layer mapping is accepted, the model's `Lcl
Translation/Rotation/Scaling` + `PreRotation` chain is applied, skin weights come from the `Cluster`
deformers with bones matched by model name, ASCII FBX is refused), sections match material slots by
name (object path or leaf) else by order. Static meshes: 16-bit index buffer (≤ 65535 vertices),
`FPackedNormal` tangents from the UVs, half or float UVs as the LOD had, bounds (tight sphere), kDOP
rebuilt with the engine's compact build (same node counts as the cooker: 68 triangles → 16 nodes);
the `RB_BodySetup` (PhysX cooked convexes) is untouched. Skeletal meshes: the file's skin joints
must be named like `RefSkeleton` bones (unknown joints are dropped from the weights), vertices are
chunked so a chunk maps ≤ 75 bones, all vertices are "soft" (counts only, the cooked arrays stay
empty), `ActiveBoneIndices` = used bones, `RequiredBones` = used + ancestors, bounds follow
`UnMeshEd.cpp` (box doubled, bottom tucked 10%, radius = extent length) and reproduce the originals
exactly on a round trip. Round-tripping an exported mesh through the importer reproduces counts,
bounds and kDOP sizes; whether the game renders imported meshes has not been tested yet.

### Animation sequences (`anim.rs`, `anim_ops.rs`)

`upk anim info|verify|export|import|compare`; Studio `anim.*`. The codec (`anim.rs`) follows
`AnimationEncodingFormat*.cpp` of the 10897 source and `AEFVariableKeyLerp<N>` in the 8.2 binary:
per track the translation keys then the rotation keys, each block padded to 4 bytes with `0x55`; one
key is a full `FVector` / `FQuatFloat96NoW`, more keys are `[mins, ranges]` for the interval
formats, the keys, then for VariableKeyLerp a frame table (`u8` when `NumFrames ≤ 255`, else `u16`).
Facts the source does not show:

- The game pads the frame tables with `0x55` too (the source pads with zeros), and a few tables hold
  frame numbers past the end: write them as stored, never clamp. With both, every VariableKeyLerp
  sequence of 8.1 (63,904) re-encodes byte-identically (`upk anim verify <dir>`); the 222
  ConstantKeyLerp ones decode. Every shipped sequence is Float96NoW rotations + None translations;
  the encoder always writes VariableKeyLerp/Float96NoW/None (and sets the three enums, which default
  to 0 = ConstantKeyLerp/ACF_None).
- Frame `k` is at `k · SequenceLength / (NumFrames − 1)`; the frame table stores `trunc(t ·
  NumFrames / SequenceLength)`.
- **Rotation convention:** the mesh's `RefSkeleton` stores every bone's rotation but the root's
  conjugated relative to the animation keys (`anim_ops::mesh_quat`). Without it characters play
  lying on their side; a round trip through our own export/import cannot catch it (both sides
  agree).
- Tracks map to bones by `AnimSet.TrackBoneNames`; with `bAnimRotationOnly`, translations come from
  the reference pose except for `UseTranslationBoneNames`.
- Import works in model space so a tool that re-orients bones (Blender) does not matter: `delta(t) =
  W_file(t) · bind_file⁻¹`, `W = delta · bind_mesh`, locals from the parents. The file's bind is the
  skin's inverse bind matrices (glTF) or the clusters' `TransformLink` (FBX). Sampling starts at the
  first key (Blender's FBX export shifts actions to frame 1). Keys that their neighbours interpolate
  are dropped (0.0005 rad, 0.01 cm). Measured round trips, max rotation error: GLB 0.07°, FBX 0.11°,
  through Blender 0.08° (GLB) / 0.86° (FBX, Euler curves). Set Blender's scene to 30 fps and the
  frame range to the action, or Blender resamples.
- FBX export writes AnimationStack/Layer + T/R curve nodes per bone (Euler XYZ in degrees,
  unwrapped; KTime 46186158000/s).
- Whole sets: `anim.export {all}` / `anim.import {all}` (or an AnimSet as `index`) export every
  sequence of the set as one named animation each (`SequenceName`, unique within a set) and import a
  file's animations into the sequences of the same name (one undo entry). A set's sequences are its
  `Sequences` exports, else the AnimSequences inside it.
- Edits (`anim.edit`): `trim` (frame range, re-keyed), `retime` (new length, same keys), notify
  move/add (duplicates the notify object)/remove; notifies are kept sorted by time. Whether the game
  plays imported sequences has not been tested in game yet.
