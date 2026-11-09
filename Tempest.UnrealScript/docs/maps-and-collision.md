# UnrealScript: maps and collision

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Maps and Kismet (`map.rs`, `kismet.rs`)

A persistent level package (`*_P`) streams sublevels through `WorldInfo.StreamingLevels`
(`LevelStreamingAlwaysLoaded.PackageName`, a package name). Each level package has one `Level`
export whose `Actors` list the actors. Placement is properties only: `Location`, `Rotation`
(rotator, 65536 per turn), `DrawScale`, `DrawScale3D`, `PrePivot` on actors; components (exports
whose outer is the actor) carry `Translation`, `Rotation`, `Scale`, `Scale3D` relative to it. The
cooker gathers most art into `StaticMeshCollectionActor`s at the origin with no native payload: each
component's own Translation/Rotation is its placement. Blocking volumes keep their shape as
`BrushComponent.BrushAggGeom.ConvexElems` (`VertexData`, `FaceTriData`); reverb/AI volumes also keep
`Brush` → Model → Polys. Level BSP models are empty in the 8.1 maps. Light collections
(`StaticLightCollectionActor`) store one light-to-world matrix per `LightComponents` entry. Every
8.1 persistent map (63) builds its scene without error (largest 2.4 s, most about 0.5 s). Moving
writes properties through `Editor::set`; Hi-Rez's `LocationPrev`/`LocationPackedKey` are left as
they are (their meaning is unknown). Kismet: `Sequence.SequenceObjects`; ops link through
`OutputLinks[].Links[] {LinkedOp, InputLinkIdx}` and `VariableLinks[].LinkedVariables`; `ObjPosX/Y`
are stripped, the connectors keep `DrawY` (inputs/outputs) and `DrawX` (variables/events), the nodes
`DrawWidth/DrawHeight`; frames keep `SizeX/Y` and `ObjComment`. `Actor.m_bCanBaseOn` (Hi-Rez,
default True) False marks "rooftops"; on Frog Isle 195 actors (98 blocking volumes, 82 meshes of its
Side level), on Brightmarsh only 95 blocking volumes (the blog's heavy-wall findings). Matinee:
`InterpTrackMove.MoveFrame` defaults to `IMF_World`; Bazaar's gates are `IMF_RelativeToInitial`
(−216 on Z over 0.4 s), its grass Matinees reach their InterpData through `SeqVar_Named`
(`FindVarName` → a variable's `VarName`). New Kismet nodes copied from another map package keep only
their class import. Adding actors (`map::adopt_actor`, `adopt_component`): a copy is appended to
`Level.Actors` (a `TTransArray`, native) and, for NavigationPoint/CoverLink/Pylon classes, to the
level's `NavListStart/End` (`CoverList…`, `PylonList…`) chain through the previous end's
`nextNavigationPoint` (`NextCoverLink`, `NextPylon`); removing (`remove_actor`) relinks the chain
around it. Same-package copies use `Editor::clone_tree`, copies from another package
`CopyMode::Placed` (Deep for content, references to actors/levels/Kismet/reach specs and anything
else inside the source level cleared; the copy's sub-objects are always copied, never matched by
path). Checked on TD_Abyss_P (player start, collection mesh, a capture point from BMO, an
InterpActor from FB_Dragon_Ruins with its mesh/materials/textures, two removals): `upk verify` 0
mismatches, the navigation chain stays whole (72 entries), five undos return the package to clean.
`upk dump` of a level prints its three lists. New actors from a class (`map::new_actor`) follow the
cooker's layout of placed actors (TD_Abyss_P's `tgjumppad_0`): `RF_HasStack` with a state frame
`node = state = the class`, probe mask 0xffffffff, code offset -1 (the stored `LatentAction` word is
uninitialised editor memory, it varies between actors of one class, so 0 is written); components as
instances of the default object's templates (archetype = template, component header with no template
owner or name, no properties); only the actor's scalar references to its components are stored
(`CollisionComponent`, `m_mesh`…), not `Components`, which the engine instances from the templates
at load. Checked for TgJumpPad, TgTeamPlayerStart, PointLight, Emitter, Note, StaticMeshActor with a
mesh from another package: `upk verify` 0 mismatches. A class default object whose native part holds
names or references is refused. Not tested in game. Scene export instances meshes (glTF nodes
sharing a mesh; FBX models sharing a `Geometry`). flate2 is built on zlib-rs: miniz (its default
backend) writes deflate blocks with a single distance code, which zlib accepts but ufbx (Blender's
newer FBX importer) rejects as "Bad DEFLATE data" (BMO's Spawn and Clutter levels hit it); zlib
always writes two. Edits round-trip (`upk verify` on saved maps: 0 mismatches); nothing has been
loaded in the game yet.

### Brushes and collision (`brush.rs`, `collision.rs`)

What collides, confirmed in the 8.2 server: `UBrushComponent::LineCheck`/`PointCheck` call
`FKAggregateGeom::LineCheck`/`PointCheck` on `BrushAggGeom` only (Hi-Rez passes `TraceFlags & 0x200`
to PointCheck as an extra argument); `UBrushComponent::Serialize` is `Super::Serialize` +
`CachedPhysBrushData` (PhysX cooked hulls, nothing else reads them for gameplay);
`BuildSimpleBrushCollision` (which rebuilds `BrushAggGeom` from `Brush`) has one caller,
`csgPrepMovingBrush`, and `InitComponentRBPhys` is an empty stub on the server; `IsValidComponent`
wants `BrushPhysDesc`, cached hulls or `Brush`, else the component is not attached at all.
`UStaticMeshComponent::LineCheck` matches the source: `UseSimpleBoxCollision` (sweeps) /
`UseSimpleLineCollision` (traces) pick `BodySetup.AggGeom`, and with no body setup nothing is hit;
otherwise the kDOP tree. The cooker strips a volume's `Brush` once it cached PhysX hulls
(`CookBrushComponent`, not when `bDisableAllRigidBody`: reverb/AI/device volumes keep theirs). Every
`FKConvexElem` member is used at runtime (box early-out, SIMD vertex planes, face normals and edge
directions for the separating-axis tests, planes for `PointIsWithin`); `FaceTriData` only for
drawing.

`set_brush` writes what an editor-saved volume holds: `Brush` (a `Model` under the actor, `Polys`
under the model, as `csgCopyBrush` names them) with the brush BSP, `BrushAggGeom` from that BSP, and
an empty `CachedPhysBrushData` (the client cooks PhysX hulls from `Brush` when it builds rigid
bodies; the old cache belonged to the old shape). Ported literally (comments name the engine
functions); deviations: `GenerateHullData`'s hull comes from an incremental hull instead of
`NxCookConvexMesh` (done in PhysX scale like the engine: ×0.02, duplicates under 0.002 merged, ×50
back, so 480 becomes 480.00003 as in cooked data), and `SafeNormal` uses an exact reciprocal where
the editor used SSE `rsqrtss` + one Newton step (an ulp apart). `bspAddPoint` searches only the tree
(`FindNearestVertex`), never the table: `GFastRebuild` is set while the editor rebuilds geometry,
and a cooked reverb volume holds its first base point twice. FVert shadow coordinates (uninitialised
in shipped maps) are written as zero; `iVertexIndex`/component indices 0 as in every cooked brush.

Checked against the 8.1 client (63 persistent maps + their
`_Collision`/`_AI`/`_Gameplay`/`_Perf`/`_Audio` levels, `examples`-style harness not kept): the
post-hull part of `GenerateHullData` (edges, face normals, planes, box, permuted vertices)
recomputed from each of 14 118 cooked convex elements' own `VertexData` + `FaceTriData` gives
identical counts, identical `PermutedVertexData` and `ElemBox`, values within 2.4e-7 (directions) /
6e-5 relative (plane W). `RebuildBrush` from the stored polys of 1 847 kept brushes: nodes 1 845
identical (plane, pools, surfaces, children, collision bound, vertex count, flags, leaves), vectors
1 846, leaf hulls 1 835, `iLink`s 1 846, bounds 1 845, points and surfaces 1 711; 746 match when the
point table is searched (`GFastRebuild` off: a volume added from the builder brush and not rebuilt
since), 1 845 one way or the other, so both histories exist in shipped maps and the rebuild-time one
is written. `KModelToHulls` from the rebuilt BSP: 1 824 of 1 847 give the cooked hulls' vertex sets
(within 0.01); in the 23 others PhysX dropped a hull vertex the plane clipping keeps. Triangulation
of non-planar faces differs from PhysX's, so the plane/edge lists of hulls rebuilt from scratch can
differ by one or two entries. New volumes on TD_Abyss_P (box, cylinder trigger, hollow box = 6
hulls, sphere) and a cooked blocking volume of Abyss_Collision reshaped from its own faces and from
its hull points: `upk verify` 0 mismatches, every rebuilt model reads back identical to a fresh
rebuild of its polys, `map.scene` draws them. Not tested in game.
