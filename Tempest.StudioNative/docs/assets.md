# Native Studio: assets

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Packages and asset previews

Packages view (`packages.rs`, `inspector.rs`, `actions.rs`, `dialogs.rs`): package list and export
tree with keys and context menus, inspector (properties edited in place, array ops, add property,
References, Source + Compile), object actions (rename, duplicate, copy to, delete, export/replace by
kind: texture, mesh, sound/movie `media.*`, animation, raw), New package, Add to the package (from a
file with a model object, copy from another package, material instance, group), Replace with, Find
and replace, Batch import, Discard, Check (`package.verify`). Previews: texture, mesh (`mesh.rs`
viewport over `mesh_view.rs`: wgpu off-screen into a canvas; LODs, textures, wireframe, Fit) with
its material slots, material (`material.rs`: sphere, parent chain, textures with Change…/From a
file…, scalars and colours edited, New instance; slots: Change…, Make it this mesh's own, Own
materials for every slot, add/remove slot), sound (`sound.rs`: waveform, rodio playback via
`src/audio.rs`, editor trim/fades/gain/normalise/channels/quality → `sound.replacePcm`), Wwise
(`wwise.rs`: AkEvent/AkBank media, play/export/replace), animation (`anim.rs`: `anim.preview` GLB
skinned on the CPU in `MeshView::set_time`, play/scrub/speed/loop, notifies, mesh and sequence
pickers).

## Creation wizard and asset interaction

**Create** (⌘N, header button, `wizard.rs`): one stepped wizard for everything new: texture / static
mesh / skeletal mesh / sound / UI movie from a file (drop or pick; `object.add` modelled on an
object of the kind, offered from the open package then searched), material instance (parent, then an
image per texture parameter, each a new texture made like the one it replaces +
`material.setTexture`), copy of any object (`object.copy` + rename), package, group, and game
content (skin, ability, card, item, champion: `mcts.create` with the champion and template picked by
name from `mcts.choices`, a backend method listing bots (champions = Champion Vendor 475 entries), a
bot's devices, items or skin collections with INT names). The package `+`, Add to the package, New
package, Add group and the Assembly's New content open it. **Scripts** section: a Lua script
(`Starter`: game event, console command, key binding or empty; written as `<name>.lua` into the open
project's `mod-files/scripts/`, else a folder picked on Create, then revealed) and an UnrealScript
class (name, parent, script package, TgGame first; `script.compile {create: true}`: the package's
decompiled sources plus the new class are compiled, the rebuilt package replaces the open one as an
unsaved edit, `added` is the new class's export and its Source tab opens). The wizard window has a
fixed size (880×720; the step body scrolls). In the object tree a group (folder) opens on a click
and closes on the next click of the selected group; other objects with children still toggle on a
double click. Inspector header: class icon, name (double-click renames), class and group, Export /
Replace and a ⋯ menu; Export and Replace ask their options first (format, mip, LOD, every sequence;
compression, mips, alpha, padding) in `actions.rs`. Raw engine data is folded under Technical
details; empty `None` placeholder exports are left out of the tree; export rows have class icons and
readable class names (`pretty_class`, `class_icon`). Texture preview (`texture.rs`): Fit / 1:1 / 2×,
any stored mip, dark / grey / light background, size, format and where the mip lives. Mesh viewport:
the wheel zooms without scrolling the pane, the background follows the theme. **Audio** view (⌘4,
`audio_view.rs`): every `.pck` of the folder, its events by name, the media of one event or all,
Play / Export / Replace. Package info (package context menu). A linked recovery workspace file every
2 minutes with unsaved edits outside a project (`recovery/<hash>.twrk`, offered in a banner when the
folder opens). A `.upk` argument opens its folder and the package. Assembly toolbar: New content,
row add/duplicate/delete icons, ⋯ (Skins, Champion vendor, Export mod folder, Undo, Discard).
Animation preview: notifies listed by class and name (Go, To the playhead, Copy at the playhead,
Remove) and dragged on the timeline (`anim.edit` notifyMove/notifyAdd/notifyRemove), **Edit the
sequence…** (trim frames, new length, RateScale), Replace asks which animation of a file holding
several (`anim.files`, `pending_anim_pick`). Mesh and animation viewports: **Grid** (at the mesh's
feet, round step) and **Skeleton** (the first skin's joints, posed with the animation) as line lists
drawn by a second pipeline in `mesh_view.rs`. Script **Source** is highlighted with gpui-component's
syntect backend (`syntect` feature, the Java grammar). Assembly grid: ⌘/⇧-click marks rows (an edit
sets every marked row, `mcts.setMany`; Delete deletes them, `mcts.deleteMany`), ⌘C copies rows as
JSON (`mcts.rowFull`), ⌘V appends them with fresh ids (`mcts.addRow {row}`), Copy as tab-separated
text, a column chooser (eye button, hidden columns per table in `assembly-columns.json`).

## Particle preview and editing

**Particles** (`studio/particle.rs`, simulation `src/particles.rs`, drawing `src/particle_view.rs`):
any ParticleSystem, emitter, LOD level or module export previews the whole system
(`particle.system`; selecting another export of the same system keeps it running and points the
panel at it): the Electron `particles.ts` simulation ported (same modules, same mulberry32 seeds,
unit-tested), sprite emitters as instanced quads (camera-facing or axis-locked, base texture,
sub-UV, one pipeline per blend mode: translucent, additive, modulate, masked, opaque), mesh emitters
as an instanced lit mesh from `mesh.preview` (the required module's texture with
`bOverrideMaterial`), on the mesh viewer's device (`mesh_view::draw_frame`/`view_proj`/`grid` are
shared; `mesh::Scene` makes the viewport generic over `MeshView` and `ParticleScene`). Play/Pause,
Restart, speed, Loop, Grid, Fit (framed once 0.6 s in); the emitter list (show/hide in the preview
only, counts, a "partial" mark whose tooltip lists the modules not simulated) scrolls on its own;
modules of the chosen emitter (double-click selects it in the package); the chosen module's
distributions as curve cards: drag a point (the range is frozen while dragging), click a point to
type its value, points (resample keeping the span), time scale, start; drafts drive the preview live
(`particles::Drafts` over the system JSON), Revert, Apply (`particle.setTable`, the module's JSON
replaced by the reply, unsaved like any edit; undo reloads the preview). Driver: `particles
pause|play|restart|fit|step <s>|emitter <n>|module <n>|set <curve> <entry> <k> <v>|apply`;
`studio_state.particles` lists emitters with counts, the chosen module's curves, drafts and the
focused graph's points in window coordinates.
