# UnrealScript: navmesh

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Havok nav meshes (`havok/`)

`upk havok verify <dir|pkg>... [-v]`, `upk havok dump|types <pkg> [<idx>] [--full]`, `upk havok
extract <pkg> <idx> <out.hkt>`. Paladins navigates bots with Havok AI 2016.1: each map's
`HavokNavMeshActor` holds one `FUntypedBulkData` (flags 0, raw, element count = size) whose payload
is a **tagfile** (`SDKV` "20160100"); `AHavokNavMeshActor::InitializeNavMesh` (8.2 server) loads it
with `hkSerializeUtil::loadOnHeap` and takes the root container's `hkaiNavMesh`,
`hkaiDirectedGraphExplicitCost` (cluster graph), the variant named "Mediator"
(`hkaiStaticTreeNavMeshQueryMediator` → `hkcdStaticAabbTree` → `Impl` =
`hkcdStaticTree::Tree<DynamicStorage<Codec3Axis6>>`) and `hkaiTraversalAnnotationLibrary`.
`HavokNavMeshGrid` is not a tagfile (a 1 MiB UE-side bulk, mostly zeros, left opaque). Some levels
carry actors with empty bulk data (TimberMill, BogDistrict, MossGarden, the 8.2 siege maps).

Format, confirmed on every file (8.1: 71 tagfiles, 97 444 faces; 8.2 server: 85, 81 320 faces; all
rewrite byte-identically and every value re-encodes): sections are a big-endian `u32` (bits 30–31:
01 leaf, 00 container; low 30 bits: size with the 8-byte header) + 4-char tag: `TAG0 { SDKV, DATA,
TYPE { TSTR, TNAM, FSTR, TBOD, TPAD }, INDX { ITEM } }` (no `PTCH`/`THSH`/`TPTR` in these files).
`TSTR`/`FSTR`: NUL-terminated names, padded to 4 with more NULs. Varints (big-endian payload):
`0xxxxxxx`, `10` + 1 byte, `110` + 2, `11100` + 3, `11101` + 4. `TNAM`: type count (index 0 = none),
per type its `TSTR` name and template parameters (`t…` → type index, `v…` → value). `TBOD`: per type
(in stored order, not index order; a few types have no body at all): index, parent, optional-field
bits (1 format, 2 sub type, 4 version, 8 size+alignment, 0x10 flags, 0x20 members `{FSTR name,
flags, offset, type}`, 0x40 interfaces, 0x80 attribute); a body with bits 0 is a typedef of its
parent. Format low nibble: 2 bool, 3 string, 4 int (0x200 signed, size bits
0x2000/0x4000/0x8000/0x10000 = 8/16/32/64), 5 float (0x174605 f32, 0x74605 `hkHalf16` = top 16 bits
of an f32), 6 pointer, 7 record, 8 array (0x20: inline, `format >> 8` elements: `hkVector4f` is
0x428). Records list base-class members first; a type's format wins over its members (`hkArray`,
`hkRefPtr`, `hkStringPtr` carry members that are not serialised). The layout in `DATA` is the
tagfile's own: pointers, arrays and strings are `u32` item indices (so `hkArray` is 4 bytes,
`hkUlong` 8 with alignment 4). `ITEM`: 12 bytes `type | flags << 24` (0x10 object, 0x20 array),
offset in `DATA`, count; item 0 is null; strings are `char` arrays with their NUL. Items lie in
`DATA` in an order of their own (an object, then the arrays it owns, then the objects it points to:
not index order), each at its type's alignment, gaps zero, `DATA` padded to 16; the writer keeps
that order and appends new items. The only bytes no member covers are none: `Face.padding` holds the
`0xCDCD` debug fill, a member like any other. Units: Havok = Unreal / 16, same axes, Z up
(`FHavokAiWorld::AddObstacle` multiplies by 0.0625).

Objects: `Face {startEdgeIndex, startUserEdgeIndex, numEdges, numUserEdges, clusterIndex, padding}`,
`Edge {a, b, oppositeEdge, oppositeFace (hkaiPackedKey, section 0 here; 0xFFFFFFFF = boundary),
flags (EdgeFlagBits: 1 silhouette, 2 retriangulated, 4 original, 8 orphaned user edge, 0x10 user,
0x20 blocked, 0x40 external opposite, 0x80 garbage), userEdgeCost (hkHalf16)}`; faces own contiguous
edge ranges, user edges are stored after all faces' own edges, each with its own vertices (also at
the end of `vertices`), `flags` 0x14 one way and 0x34 (blocked) back, cost 1.0, `edgeData` =
`0x7F900000 + n`. Many vertices are unused (Abyss: 5793 for 497 faces). 6 opposite pairs of the
shipped meshes do not share vertices (they overlap in part). `faceData`/`edgeData` striding 1, all 0
on ordinary faces. Shipped jump links are `UserEdgePair`s whose `x`, `y`, `z` are SoA: the four
points are `(x[k], y[k], z[k])`, A's segment then B's; their annotations are all
`HKAI_TRAVERSAL_TYPE_DROP_DOWN` on Abyss. An unused annotation library keeps uninitialised `up`.
Static tree (`tree.rs`, `hkcdStaticTree::Codec3Axis::unpack`/`pack`, 1/226 = 0x3b90fdbc): 6-byte
nodes depth first, `xyz` nibbles squared × (parent extent / 226) in from the parent's decoded box,
`hiData & 0x80` internal (left child next, right child `2 × data` further, data = left leaf count),
else face index; the root's box is the `domain`, whose w lanes are the min/max of the vertices' w
(hkAabb min/max on four lanes).

Building the tree (`build.rs`): `hkaiStaticTreeNavMeshQueryMediator::setNavMesh` inserts every
face's `calcFaceAabb` (no radius) into an `hkcdDynamicAabbTree`, calls `rebuild()` =
`rebuildBranchSAH(root, true, 32 bins, 16)` and `hkcdStaticAabbTree::buildFromDynamicTree`. Ported
and checked against the stored trees: `calcFaceAabb` on four lanes (every rebuilt domain equals the
stored one, w lanes included), the binning (bounds of the leaves' boxes, `trunc(clamp((centre − min)
× (1/extent × 32) + 0.5, 0, 31))`, constants 0.5/0/1 from `data_32bd510`/`410`/`420`), the per-bin
cost (left and right sweeps both including the bin, area = `t.z + (t.y + t.x)` of `d.yzx × d`,
strict `<` from 0x7f7fffee), bins below the best to child 0 and above to child 1 (disassembly at
0x2d71536), the best bin sorted with `explicitStackQuickSort` on the axis centre and halved (or
given whole to an empty side), the brute-force pairing below 17 leaves, and `buildFromDynamicTree`
(preorder copy, packing against decoded parents, `(child1 − i − 1) >> 1`). Every rebuilt root node
equals the stored one (71/71 on 8.1) and the partitions match further down (on TD_Abyss_P the first
16 nodes match once child 0 is the bigger side, the leaf sets of nodes 1 and 2 are the stored ones),
but no tree is byte-identical yet: below the root the stored trees sometimes put the upper side
first, and the order of leaves inside a branch (which decides brute-force ties and child order)
comes from the incremental `hkcdDynamicAabbTree::insert` tree, which is not ported (the rebuild
takes the leaves in face order). `upk havok verify` reports the count (`AABB trees rebuilt
identical`), `HAVOK_TREE_DEBUG=1 … -v` prints the nodes around the first difference and the
`debug_split` test (`HAVOK_TREE_FILE=<extracted payload>`) shows where a stored split falls in the
bins. Edits use this build, so an edited mesh gets a valid tree covering every face, not necessarily
the one Havok would build. Editing (`edit.rs`): delete faces (edge ranges compacted, opposites
renumbered or made boundaries, user edges to deleted faces dropped, face/edge data, library pairs
renumbered), move vertices (mesh AABB grown), then `write_back` re-encodes the touched arrays over
their stored elements, rebuilds the tree and drops `clearanceCacheSeedingDataSet` (its items stay,
unreferenced). Checked on TD_Abyss_P (3 faces deleted, a region cut, a vertex moved, saved copy):
`upk verify` 0 mismatches, `upk havok verify` valid, three undos return the package to clean. Not done for edits: rebuilding the cluster graph (`hkaiNavMeshClusteringTask`); generation (below)
builds its own. Not tested in game.

### Generation (`havok/generate.rs`, `map.navmeshGenerate`)

Not Havok's generator (the 8.2 server links `hkaiNavMeshGenerationUtils`; not ported): Recast
(`rerecast` 0.5.0, pure Rust) over what pawns collide with, made into a mesh with the shipped
meshes' conventions. Input (`serve.rs` `walk_geometry`, the map and its streamed levels): the world
BSP (`ULevel::Model` node polygons, `PF_NotSolid` surfaces left out; the older SG_T* maps are BSP),
blocking volumes' `BrushAggGeom`, and each static mesh component blocking pawns
(`collision::flags_json`) with its body setup's `AggGeom` (boxes as boxes, spheres and capsules as
their boxes, `collision::agg_triangles`) or its LOD 0 triangles under complex collision; a mesh
without a body setup blocks nothing and is left out. Instanced (foliage) components are skipped.
Settings (Havok units): cell 0.75 × 0.25 (12 Unreal units across; 8 cost 3× the time on Bazaar for
no more coverage: watershed regions dominate), agent radius 2.0625 (the shipped `erosionRadius`),
height 11, climb 2.1875, slope 45°, up to 12 vertices a face. The voxel box is the current mesh's
grown by 8 radii (else the walkable triangles' extent): far-off geometry overflows Recast's span
heights. Each contour is cut into polygons on its own (`rerecast` fails the whole mesh on one it
cannot triangulate; C++ Recast skips it). Faces are counter-clockwise seen from +Z, edges chained,
opposites paired by vertices, all `ORIGINAL`, `padding` 0xCDCD, face/edge data 0, the AABB grown by
the radius. Pruning: connected parts with no seed go (seeds: `NavigationPoint` actors and the current
mesh's face centres, unless `keepUnreached`): Recast keeps roof tops, wall tops and closed
interiors. Clusters grow breadth first to 400 units² (the shipped graphs: node near the cluster
centroid, costs ≥ the straight distance, both ways, `hkHalf16`); our cost is the distance. The tree
is rebuilt; the tagfile is stored as a read and rewrite gives it (canonical: `upk havok verify`
passes).

Jump links (`havok/links.rs`; `keepLinks`, `findLinks`, both default true; `generated.links`
counts them). Layout as shipped (checked on TD_Abyss_P): a pair's user edges come after every face's
own edges, each face's contiguous; the one on face A (take-off, points 0-1) is `USER | ORIGINAL`
(0x14) with `userEdgeCost` = `costAtoB` (1.0 = `0x3f80`, a truncated float), the one on face B
(landing, points 2-3) points back and is also `BLOCKED` (0x34, cost 0) when `costBtoA` is 0; each
has two vertices of its own after the faces' vertices, A's then B's per pair. Annotation `k` belongs
to pair `k` (`userDataA` = `0x7F900000 + k`); its `tEquation` maps the take-off segment to t 0..1
(`(d.x, d.y, 0, −d·a0) / |d|²` in the plane). The shipped edge data of user edges is scrambled
(Havok's insertion order; the A edge does not always hold `userDataA`); ours does. `validate` checks
user edges too (all 70 shipped meshes pass). Kept links: each segment goes onto the new face nearest
its middle (within 2 erosion radii across and 3 units up or down), its ends clamped onto that face,
`tEquation` recomputed; both ends on one face means the new mesh walks across (Recast bridges many of
the small gaps the shipped jumps cross: "not needed"), an end on no face loses the link. New
drop-downs follow the shipped ones (5467 on 8.1: horizontal reach 11.85 for nine in ten, drop 1.8-50,
width 0.2-50): from each boundary edge, every 0.5, the highest face 11.85 out and `maxClimb`..52 lower,
clear of the level's collision (rays at 1 and half the agent height out, then down); runs over one
landing face make one link (≥ 0.5 wide, ≤ 50), skipped within 4 of a kept link's take-off; one way,
cost 1, `DROP_DOWN`. Cluster graph edges are added from the take-off face's cluster to the landing
face's. An unused library's uninitialised `up` is set to +Z when links are added. On all 69
generatable 8.1 maps: of 7808 shipped links 6338 kept, 1112 not needed, 358 lost (Serpent Beach the
most: 36-41), 8357 drop-downs found; every result valid, `upk havok verify` 0 failed. Horizontal
jumps (type 0) are kept, not found.

Checked on the 69 shipped 8.1 meshes (`map.navmeshGenerate` then `package.save` to a scratch copy):
every result valid, `upk havok verify` 0 failed, `upk verify` 0 mismatches, 83 s for all (Bazaar 17
s). Coverage (share of one mesh's area whose face centres lie on the other): median 93 % of the
shipped mesh covered and 92.5 % of ours on it, 44 maps ≥ 90 % both ways. Low ones are explained:
Dragon Ruins streams `ruins_*` levels that are not in the folder (`missingLevels`), FT_P_v01 and
FMM_P_v01_SiegeOnly ship meshes over areas with no geometry left (stale); Marauders, FishMarket and
IceMines are at 75–80 % (not looked into). Not done: finding new jumps (only drop-downs), Havok's
own clustering, actors with empty bulk data (TimberMill, MossGarden: a template
tagfile from another map would do). Not tested in game.

