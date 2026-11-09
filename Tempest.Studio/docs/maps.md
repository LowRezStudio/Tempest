# Studio maps, Matinee, collision, lighting and navigation

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- **Maps** (Map view, Ctrl+4; `src/lib/studio/map.svelte.ts`, `components/map/`: `MapSide`,
  `MapViewport`, `MapInspector`, `KismetGraph`, layout in `src/lib/studio/kismet-layout.ts`; backend
  `map.rs` + `kismet.rs`, `map.list|scene|setTransform`, `kismet.sequences|graph|link`): pick a
  playable map (packages that stream sublevels) and the Studio reads it and every streamed sublevel
  (`WorldInfo.StreamingLevels` → package names) as actors with world matrices (three.js
  column-major, in the mesh previews' metre/Y-up space: `P⁻¹·M·P`) and what they draw: meshes (one
  `mesh.preview` per mesh and component material set, `materials` overrides, 512² textures, two
  requests in flight, clones per placement), brushes (`BrushAggGeom` convex elements, else the
  brush's polys), cylinders, lights (light collections place theirs with stored matrices), particle
  and actor markers; categories (player starts by team colour, spawns, factories, gameplay,
  triggers, blocking/other volumes, lights, sounds, emitters, navigation) and levels can be hidden;
  shading lit/unlit/untextured/wireframe; left-drag orbit, right-drag look (the orbit target turns
  around the camera) + WASD/QE fly by physical key (`KeyboardEvent.code`, so ZQSD on AZERTY), arrows
  and Page Up/Down fly without the mouse, middle-drag pan, wheel zoom, click picking, F frames,
  W/E/R/Q gizmo (`TransformControls` on a proxy). Releasing the gizmo writes the actor's
  `Location`/`Rotation`/`DrawScale3D` (`map::set_transform`, rotator from `FMatrix::Rotator`,
  PrePivot honoured), or for a mesh of a `StaticMeshCollectionActor` its component's
  `Translation`/`Rotation`/`Scale3D`; one undo step, an ordinary dirty package (map undo goes
  through `package.undo` of the edited level, Save saves the map's dirty levels,
  `studio.saveAll(only)`). The inspector is `PropertyTree` with `path`/`onchange` (it and
  `AddPropertyPicker`/`ValueNode` take the package explicitly). Kismet: the level's sequences
  (breadcrumb, sub-sequences open by double-click), nodes laid out from the connectors'
  `DrawY`/`DrawX` (the cooker strips `ObjPosX/Y`), then next to linked nodes, variables under their
  connector; drag output→input or variable connector→variable to link, Alt+click or select + Delete
  to unlink (`kismet.link`), node positions dragged by the user kept in the store only. **Heavy
  walls** (toolbar): actors with `m_bCanBaseOn` False (`canBaseOn` in the scene, class default when
  absent; a collection mesh takes its collection actor's) are drawn with a slope shader (walls red =
  heavy walls, slopes orange, roofs grey; it must include three's `logdepthbuf` chunks because the
  view uses a logarithmic depth buffer), flagged volumes shown see-through even when volumes are
  hidden; **X-ray** (`map.heavyStyle`) draws each flagged object one flat colour with `depthTest`
  off and a high render order, so it shows through walls; the outliner's **rooftops** filter, the
  inspector's **Can be stood on** switch (`map.setCanBaseOn`). **Export** (`MapExportDialog.svelte`,
  `map.export`): the levels and kinds shown as one GLB (`mesh_ops::write_scene_glb`) or FBX
  (`mesh_fbx::write_fbx_scene`, sharing `geometry_node`/`finish` with the single-mesh writer): each
  mesh once (keyed by mesh and component materials), a node per placement under a node per level,
  textures up to a size, optional volume brushes; `map.progress` events; Abyss in about 3 s, checked
  by importing both in Blender (528 objects over 93 meshes). Kismet **adding**: `kismet.palette`
  (every SequenceObject class the workspace uses, from the search index, with an example, preferring
  the level's own), `kismet.add` copies the example (`duplicate` in the same package,
  `CopyMode::Clear` from another, so nothing of another map package is imported), then
  `kismet::adopt`: outer/`ParentSequence` the sequence, links, originator, `ObjName`/`ObjComment`
  cleared, connector `DrawY`/`DrawX` set for the requested position, appended to `SequenceObjects`,
  an event's originator (the actor selected in the 3D view) gets it in `GeneratedEvents`, an object
  variable its `ObjValue`; `kismet.remove` drops the node, every link to it and its originator's
  entry, then deletes the export. UI: right-click or A opens the palette, Delete removes the
  selected node. **Matinee** (`matinee.rs`, `matinee.get|setKey|addKey|removeKey`,
  `MatineePanel.svelte`, `matinee-math.ts`; map view `matinee`): a SeqAct_Interp's InterpData
  (through its "Data" connector, `SeqVar_Named` resolved by `VarName`), groups with the actors
  linked to the connector of the group's name, every track's key lists read generically
  (`<Prop>.Points` curves, arrays of keys with `Time`), split move axes (`SubTracks`); keys and the
  whole timeline edited (see **Matinee editor** below); the preview evaluates like the engine, moves
  the driven actors' nodes and draws each movement's path. **Adding and removing actors** (toolbar
  **Actor** / Shift+A opens `ActorPalette.svelte`, `map.palette`: every actor class the workspace's
  levels place, with an example preferring the target level, then the map's levels; inspector or
  Ctrl+D duplicates, Delete deletes, both commands in `actions.ts`): `map.addActor {path, source,
  parent?, matrix? | at?}` copies an actor into a level, placed where the view looks (`at`, the
  orbit target, `map.viewTarget`): from the same package `Editor::clone_tree` (the actor and its
  sub-objects, references among them remapped, others kept), from another package
  `copy_with(CopyMode::Placed)` (content such as meshes, materials and textures copied along,
  references to that level's objects cleared; an import whose outer is a group the source exports,
  e.g. `ab_materials.mat.mat_x` in a seek-free level, keeps that outer chain, and content the target
  already exports at the same path is used as that export: an import without its outer names a
  package, and the engine then loads a `UPackage` as the material and crashes in
  `UMaterialInstance::GetMaterial`; light and shadow maps always come as new objects, renamed
  `<name>_copy<n>` when the name is taken, and every copied texture whose engine path changes,
  top-level light maps above all, gets its texture-cache mips inline, since
  `MergedFileIndexCache.bin` is looked up by that path: "Could not get entry for texture …"
  otherwise); `map::adopt_actor` gives it a fresh name, appends it to `Level.Actors` and the level's
  nav/cover/pylon lists (`nextNavigationPoint`…), drops `PathList`, sets `VisibilityId` -1 on its
  primitives (precomputed visibility belongs to the original's place), NetIndex -1; with `parent` a
  collection mesh is cloned into the same collection (`StaticMeshComponents`). The palette's **New**
  tab builds an actor from its class instead (`map.addActor {path, class, mesh?, at?}`, the classes
  from `map.palette`'s `placeable`: `ClassDb::placeable_actors`, volumes included (`volume`, shaped
  by `collision::set_brush`, see **Collision and volume shapes**), other brushes left out;
  `map::new_actor`): laid out like cooked actors: no archetype (the class default object is implied;
  an archetype import of `Default__<Class>` makes the game fail with "Failed to load
  ObjectArchetype"), an empty state frame, one component instance per template of the default object
  (archetype the template, an import `engine.default__<class>.<template>` of class package `engine`;
  no properties), the default object's references to its templates pointed at the instances (tagged
  `ObjectProperty`: no content package has a `ComponentProperty` tag, and every property the Studio
  creates follows that), `Tag` the class name, everything else the class defaults; its **Mesh** tab
  places a StaticMeshActor showing a mesh picked anywhere (`mesh`, copied in deep from another
  package). `map.removeActor {path, index, parent?}` (`map::remove_actor`): out of `Actors` and the
  lists (the chain relinked), then `Editor::delete` (references cleared, Kismet included); WorldInfo
  and the builder brush are refused. One undo step each. Not rebuilt automatically: baked lighting
  (copies keep the original's light maps until **Bake lighting**, below, rebakes them), the Havok
  nav mesh (edit it with **Nav mesh**, below; nothing regenerates it), the path network, cached
  PhysX data for new meshes/scales (cooked at load), BSP; Hi-Rez's `NetworkId` is copied as is, and
  a new actor exists only in the client package unless the server's map has it too. A map is a place
  (`MapPlace`: map, level, export, view, sequence) of the history and tabs; the mod cooker is one
  too (`ModPlace`), and switching views records a move, so a tab keeps the view it was left on.

- **Matinee editor** (Map view's Matinee timeline, `MatineePanel.svelte`,
  `MatineeCurveEditor.svelte`, stores `src/lib/studio/matinee.svelte.ts` (`matineeEd`) and
  `map.matinee*`, preview `matinee-preview.ts`, engine evaluation `matinee-math.ts`; backend
  `matinee.rs` + `matinee_curve.rs`,
  `matinee.get|list|create|setKey|addKey|removeKey|addGroup|removeGroup|setGroup|addTrack|removeTrack|set|connect`):
  opened by double-clicking a SeqAct_Interp in Kismet, the timeline's picker (the level's Matinees),
  or the commands **Open the level's Matinees** / **New Matinee** (in the selected actor's level,
  with a group and relative move track driving it). The tree adds, renames (double-click), recolours
  and removes groups (one director group: cuts, fades), adds tracks of every class the game uses
  (movement, event, Wwise event, sound, animation, toggle, visibility, fade, director,
  float/vector/colour property, material parameters) and removes them, toggles `bDisableTrack`, and
  links the actor selected in the view to a group (or unlinks it). Keys: click selects, drag retimes
  (Alt: past the neighbours, re-sorted), double-click a row adds, K adds at the playhead
  (`map.matineeKey`; a move key from where the actor is drawn), Delete removes; the strip below
  edits the key's time, value, interp mode and tangents (Auto recomputes them) or a timed key's
  members (event names, cut targets, toggle actions, sounds), or the track's MoveFrame, RotMode,
  look-at group, raw initial, title, property; **Action** edits the SeqAct_Interp's flags, PlayRate,
  start position and the section. The curve editor (Activity button on a curve row) draws each
  component with the engine's evaluation, drags keys (Shift: value only, Ctrl: time only) and
  tangent handles (an auto key becomes CIM_CurveUser, a CIM_CurveBreak key moves one side), wheel
  zooms, F fits. Playback loops, plays only the section, and runs at the action's PlayRate or a
  preview rate. The preview (`timelinePreview`) moves driven actors exactly like the game
  (`IMF_RelativeToInitial` with the initial transform less the first key, split axes,
  `bForceStartPos`), scales them by DrawScale3D tracks, tints light markers by LightColor/Brightness
  and hides what toggle/visibility tracks switch off; in this view a gizmo move of a driven actor
  keys its movement (the key under the playhead, else a new key) instead of moving its placement
  (`map.captureMove`). Undo re-reads the timeline. MCP `studio_map`: `matinees`, `newMatinee`,
  `addGroup`/`removeGroup`/`setGroup`, `addTrack`/`removeTrack`, `setKey`/`addKey` (mode, tangents,
  fields, values), `setInterpMode`, `setTangents`, `keyAtPlayhead`, `setProps`, `connect`,
  `matineeTime` (poses, hidden, tints). What the 8.1 maps store and the engine rules ported:
  [backend Matinee reference](../../Tempest.UnrealScript/docs/matinee.md).

- **Collision and volume shapes** (toolbar **Collision**, command `map.collision`;
  `src/lib/studio/collision.svelte.ts`, `components/map/CollisionToolbar.svelte`,
  `CollisionPanel.svelte` in the inspector; backend `brush.rs` + `collision.rs`,
  `map.collision|brush|setBrush`, MCP `studio_map` `collision`/`brush`/`setBrush`): draws what pawns
  and shots hit over the render geometry (or alone, **Only**), coloured by what it blocks
  (`flags.blocks`: all, pawns = box sweeps only, shots = line traces only, none; volumes that block
  nothing by kind, triggers green): volumes as their `BrushAggGeom` convex hulls, static meshes as
  their simple collision (`BodySetup.AggGeom` boxes, spheres, capsules, hulls) or, when the mesh
  uses complex collision, its LOD 0 triangles (an untextured `mesh.preview`). `map.collision` costs
  about 0.1 s on Abyss (all ten levels, 800 KB of JSON). The inspector's **Collision** section shows
  the actor's and component's switches as toggles (`object.set`) and what a mesh's pawns/shots test;
  a volume gets a **Shape** panel: UnrealEd's builders (box, cylinder, cone, sphere with their
  parameters and defaults, hollow variants), a mesh's convex hull or bounds, or its vertices edited
  as a table (the volume becomes their convex hull), applied with `map.setBrush` (one undo step).
  New volumes (any `Volume` class of the palette's New tab) are built with a 256 cube unless `brush`
  is given. Facts behind it (8.1 client data): every `StaticMesh` keeps the intrinsic defaults
  `UseSimpleBoxCollision`/`UseSimpleLineCollision` TRUE (no package stores the names), so a mesh
  without a `BodySetup` collides with nothing (Abyss: 23 of 84 meshes); cooked blocking volumes
  store `BlockZeroExtent` False (they block pawns, not shots), while the BlockingVolume template
  says True, so a new one blocks both until switched.

- **Bake lighting** (map toolbar **Light**, command `map.bakeLighting`; `LightBakeDialog.svelte`,
  `src/lib/studio/lightbake.svelte.ts`; backend `lightbake/` in tempest-assets, `light.bake` /
  `light.info`; MCP `studio_map` `bake` / `lightInfo`): static light maps for the meshes **added or
  moved this session** (`scope: "edited"`: the component or, outside collections, its owner is an
  unsaved change), the **selected** mesh or actor, or whole **levels** (the dialog warns about
  time). Lights and shadow casters come from every level of the map; direct light (point, spot,
  directional, sky) is ported from Lightmass, indirect light is an optional one-bounce approximation
  (`indirectRays`, a constant albedo times the world's `DiffuseBoost`, `EnvironmentColor` for
  escaped rays, the world's ambient occlusion), so shaded areas come out darker than Lightmass
  without it. Every LOD of a mesh gets its own allocation of the component's size, from that LOD's
  light map UVs (shipped components hold one per LOD in the same atlas; only LODs without light map
  UVs are cleared, `clearedLods`). The bounce albedo defaults to 0.08, fitted on Shootingrange's
  Courtyard_ABC and Room_D (48 rays: shaded meshes at 0.96-1.03 of the shipped mean, sunlit
  0.87-1.26, per mesh within about ×1.6; direct-only shade is about 0.6); escaped rays take
  `EnvironmentColor` × `environmentScale` (1). Results are written like the editor's encoder writes
  them (new `NormalizedAverageColor`/`DirectionalMaxComponent` `LightMapTexture2D` atlases, mips
  inline, the components' `FLightMap2D` pointed at them), then every light/shadow map of the package
  nothing uses any more is deleted (`removed`: the replaced ones, and orphans an older Studio left
  from copies), as one undo step per level, `light.progress` events while it runs, then the levels
  reload; the dialog shows the selected mesh's light map (lights, textures, scales, a decoded
  irradiance PNG with an exposure slider) and, with **Compare**, each mesh's mean irradiance against
  the light map it had. Measured on TD_Abyss_P (8.1): one wall in 0.4 s, Abyss_Art_Walls (187
  meshes, 1.04 M shadow triangles, 101 lights) in about 15 s direct-only. Not shown in the 3D view
  yet (no lighting-only shading mode); not tested in game.

- **Nav mesh** (map toolbar **Nav mesh**; `src/lib/studio/navmesh.svelte.ts` store,
  `navmesh-three.ts` objects, `components/map/NavMeshPanel.svelte`; backend `map.navmesh {path,
  index?}` / `map.navmeshEdit {path, index, op, faces?, moves?}` in `havok/studio.rs`): the
  persistent level's `HavokNavMeshActor` tagfile (Havok AI, not UE3 paths) drawn over the scene:
  faces coloured by region (faces joined through opposite edges; jump links do not join) or cluster,
  every edge once (boundaries red, user edges cyan), the jump links
  (`hkaiUserEdgeUtils::UserEdgePair`s) as arcs between their two edges, X-ray. Buffers come as
  framed binary payloads (positions, fanned triangles + face per triangle, cluster/region per face,
  edge pairs + kind, link points) in view space (Havok = Unreal / 16, same axes, then `(x, z, y) /
  100`). Picking (Actors / Faces / Vertex): Shift adds faces, Whole regions, **Delete faces**, **Cut
  region** (every face connected to the picked ones), **Move the vertex** (Unreal units). Each edit
  is one undo step of the level package (`map.noteEdit`; undo reloads the level, the store
  re-reads); the backend checks the mesh stays valid, rebuilds the mediator's face AABB tree, drops
  the clearance cache seeding and writes the tagfile back into the actor's bulk data. Not rebuilt:
  the cluster graph (its nodes keep their faces' cluster indices), streaming sets (none in shipped
  maps); generation from geometry is not done. `studio_map` action `navmesh` {navOp:
  status/show/hide/pick/deleteFaces/cutRegion/moveVertex, faces, vertex, location, colour}; the
  store is `navmesh` in the MCP eval scope. Not tested in game.
