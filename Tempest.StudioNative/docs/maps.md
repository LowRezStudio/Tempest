# Native Studio: maps

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Map editor

**Maps** (⌘5, `studio/map/`: `mod.rs` the `MapEditor` entity, `scene.rs`, `viewport.rs`, `side.rs`,
`panel.rs`, `kismet.rs`, `matinee.rs`, `curve.rs`, `collision.rs`, `bake.rs`, `navmesh.rs`,
`palette.rs`, `tool.rs`; renderer `src/map_render.rs`; property editor for any export
`studio/props_view.rs`): the Electron map view at parity. `map.scene` levels become nodes of
`MapScene` (layer 0 the actors, 1 collision, 2 the nav mesh): meshes loaded once per mesh and
material set (`mesh.preview`, two in flight, GLBs parsed on a background thread, instanced per
placement; a mesh that fails is a red box), brushes as see-through fills + feature edges, cylinders,
lights and markers as built-in shapes; drawn off-screen with wgpu on the mesh viewer's device with a
reversed depth buffer (`draw_frame_depth`), at half resolution while the camera moves
(`settle_later`), only when something changed; shading lit/unlit/untextured/wireframe, heavy walls
by slope in the shader or X-ray over everything. Camera: left drag orbits, right drag looks (WASD/QE
fly while held), arrows/Page Up/Down fly, Shift faster, middle drag pans, wheel zooms; F frames,
Escape deselects. Picking on the CPU (`MapScene::pick`: meshes by their triangles, volumes by their
edges). Gizmo (`viewport.rs`, W move with axis and plane handles, E rotate rings, R scale axes +
uniform, Q select only): drawn as lines over everything, dragged in screen space, written on release
(`map.setTransform`, or in the Matinee view a key of the driven actor's move track, `capture_move`).
Side bar and inspector widths are kept in the settings (`map_panes`; sized panes are `flex_none` and
the middle has a zero basis, else the panes came out narrower than their sizes). Side bar: map
picker (playable only), levels (show/hide, unsaved dot, Kismet), categories with counts (hidden ones
kept in `prefs.map_hidden`), outliner (virtual list, filter, rooftops only, double-click frames).
Inspector: actor header (frame, duplicate, delete, open in the package view), component note, Can be
stood on, location/rotation, info and parts, the Collision section (switches as toggles,
pawns/shots/no body setup, a volume's Shape panel: box/cylinder/cone/sphere builders, from a mesh,
edit vertices as `x y z` lines), then `PropEditor` (any package's export: edit in place, array ops,
reset, add a property). Kismet (`kismet.rs`): `layout` ported from `kismet-layout.ts` (unit-tested),
nodes drawn as divs over a painted canvas (dot grid, bezier links), all hit-testing in the editor:
pan, wheel zoom, drag nodes (view only), drag an output to an input or a variable connector to a
variable to link, Alt+click or Delete a selected link to unlink, double-click a
sequence/SeqAct_Interp, right-click or A opens the node palette, Delete asks; node inspector with
Fired by. Matinee (`matinee.rs`, `curve.rs`): the timeline's toolbar (picker, New, start, play,
loop, section, rate, length, Action settings, add group, Director), groups (colour as hex, rename,
drive/stop driving the view's selection, add track by class and property, remove), tracks (enabled,
curve editor, K, remove), lanes (scrub, drag keys, Alt past neighbours, double-click adds), the
key/track strip (time, values, interp mode, tangents, Auto, timed-key fields; MoveFrame, RotMode,
look-at group, raw initial, title, property), the curve editor (engine evaluation, drag keys with
Shift/Ctrl, tangent handles, wheel zoom, Fit); the preview (`matinee::preview`) poses driven actors,
scales, tints lights and hides toggled actors with `tempest_assets::matinee_curve` and
`map::{to_view, from_view, decompose}` (unit-tested round trip). Collision overlay, Light bake
dialog (scopes, rays, albedo, compare, the selected mesh's light map with an exposure slider), Nav
mesh panel (colour by region/cluster, X-ray, pick actors/faces/vertex, whole regions, delete faces,
cut region, move a vertex), actor palette (Copy / New / Mesh with a `workspace.search` of static
meshes, into a chosen level), Export dialog. Every edit is an unsaved edit of its level
(`note_edit`: the map's own `dirty` copy, the Studio told on save/discard by `forget_map_edit`), ⌘Z
in the Map view takes back map edits first, Save writes the map's unsaved levels. Places:
`Place::Map` (map, selection, view, sequence, Matinee). Commands: Show the maps, Add an actor, Show
collision, Bake lighting, New Matinee, Open the level's Matinees, Matinee key at the playhead;
⌘D/Delete act on the map's selection there. MCP `studio_map` (`tool.rs` + `mcp::map_call`): the
Electron tool's 43 actions with the same arguments; an action starts its work and answers once
nothing is in flight (`$read`), runs again once a list it needs is read (`$retry`), or answers with
a backend call (`$backend`); scene actions show the 3D view; `studio_state.map` (levels, selection,
camera, gizmo handles on screen, frame time, Kismet/Matinee state, errors). Driver: `map open
<name>|select <level> <export>|<name>|deselect|click x y|tool <t>|shading <s>|heavy|xray|category
<id> on|off|level <name> on|off|frame|frame-all|mode scene|kismet|matinee|kismet [level] [seq]|node
<n>|palette|node-palette|matinee <level> <export>|time <s>|play|track <n>|curve <track>
<path>|collision on|off|collision-only|nav on|off|nav-pick
faces|vertex|off|export-dialog|bake-dialog|close|undo|orbit dx dy|zoom f`; probes `map-viewport`,
`kismet-graph`, `kis-node-<export>`, `mt-lanes`, `curve-editor`, `map-<name>`;
`studio_state.map.matinee.curveKeys` gives the open curve's keys in the curve editor's points; the
driver step `probe <name>` prints a probe's bounds. `InputState::set_value` is not visible to
`value()` at once: code that sets a field's text must also apply it itself. An entity must not be
updated while it is being updated: the driver updates the Assembly/Mod entities directly, not
through `Studio::drive`. `just studio-native [<game dir>]`.
