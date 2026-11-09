# Native Studio: movies

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Movie editor

**Movies** (`studio/swf/`: `mod.rs` the `MovieEditor` entity, `stage.rs`, `timeline.rs`, `tree.rs`,
`props.rs`, `character.rs`, `sections.rs`, `tool.rs`, `geom.rs`; player `src/swf_player.rs`): the
Electron movie editor at parity. Ruffle's Rust core and wgpu renderer (v0.7.1, the same wgpu 30 as
the Studio) run on a `ruffle` thread (a player is not `Send` and takes ~1 s to make; stills: build,
12 frames, capture, drop, `transparent` for non-root targets and PNG exports via `set_window_mode`;
one live player for the Movie node and Play, `advance`/`pointer`); `scripts/fetch-ruffle.sh` clones
the tag into gitignored `vendor/ruffle` with `Tempest.Studio/ruffle/*.patch` (9-slice), run by the
just recipes, `bundle.sh` and the release workflow. A SwfMovie selects the inspector's **Movie** tab
(Properties and References stay); one editor per movie export (`Studio::movies`, made on first view,
kept while the package is open, re-read on `reload_tables`); the first one opened turns Focus on
(`Studio::focus_mode` hides the package and object lists). Layout like Electron/FFDec: toolbar
(breadcrumb Root › clips, movie info, Focus, Layout/Play for clips, Move/Transform tools, bounds,
zoom −/fit/+ /Fit, Export SVG/PNG 1/2/4×/all frames, Image, Make a mod (opens the Mod view),
Discard, Apply N edits), resource tree (virtual list: Movie, Frames, character groups after FFDec's
with counts and collapse, then Edit texts, Scripts, External images, Linkage, All tags; filter by
id/name/kind, Up/Down, follows the editor), the stage over the resizable timeline, the properties.
Stage: `swf.preview` stills with the pending edits, hidden layers as `exclude`, a fixed view per
target (`default_view`: the root's stage, else every object + the stage outline, 3 %), pictures
drawn at the stage's scale × device scale (steps of 64 px) and kept until the new one arrives
("updating view…", "Rendering…", errors, a missing-images bar); checkerboard following the pan,
dashed stage outline with its label, every object's bounds (edited ones in the warning colour), the
selection as its transformed polygon with a name/depth/position label; click picks the smallest
object (Alt+click cycles), drag moves (Shift: one axis), the transform tool's 8 handles scale (Shift
keeps proportions, Alt from the centre) and the knob rotates (Shift: 15°), live matrix until the
timeline reports it (`committed`); arrows nudge (Shift 10), Delete removes, Enter/double-click/Space
tap enter a clip, Backspace up, Escape deselects, V/Q tools; middle-drag, Ctrl+drag or Space+drag
pans, the wheel scrolls (Shift sideways), ⌘/Ctrl+wheel zooms at the pointer. Timeline: frame strip
(labels, script dots), layer actions (Hide off-stage, Solo selected, Hide all, Show all), the layer
table (eye: render-only hiding per sprite, Alt+click solos; depth with mask, name with
edited/stacked ×N (overlap ≥ 0.75)/off stage/alpha-0 marks, character with class-code mark, x, y,
scale, rot, alpha, tags), Up/Down, Left/Right frames, Enter opens. Properties: placement (revert,
Edit inside, name, x, y, scale x/y with keep proportions, rotation, skew, alpha, Show/Hide, Remove
with a second click, the placing tag line), then the character (linkage, tag, size, frames, import
source, an edit-text's initial text (textarea, commits on blur), an external image's file name and
image card: thumbnail, Replace… (`texture.import` keepAlpha/padBlocks, stretched warning), PNG, DDS,
go to the texture; the character editor (`swf.character`): shape colours and gradient stops (colour
picker, hex, alpha) and lines (width, colour, alpha), Replace with SVG (fit), static-text records
(text, colour), every edit-text property (font menu, size, colour/alpha, align, margins, indent,
leading, max length, variable, box, the 10 flags), fonts (info, characters, add glyphs from a file
with Latin-1/Latin Extended-A/Cyrillic/Greek presets, TrueType export), sound and binary export, an
export row (SVG, PNG at 1/2/4×, all frames of a sprite); Uses / Used by (sorted like the tree) and
attached tags). Tag views: texts (initial text per field), images (file rename, compact image card,
Add image…), linkage, all tags (filter); Scripts: grouped list (classes, frame scripts, clip events,
buttons; filter, Up/Down, staged dot) and the editor (`EditorState`, JavaScript highlighting):
ActionScript / P-code, the parts kept as P-code, Revert, Check (errors move the cursor to their
line), Stage (⌘Enter). Edits (`edit_key`, one per key in the order made, the Electron keys) and an
undo stack (changes of one key within 600 ms are one step; ⌘Z in the Packages view takes back the
last pending change first); Remove deletes every tag of the object, per-object revert. Add image
dialog (`swf.addImage`: file, texture name, shape, place at x/y with an instance name; pending edits
written with it). Exports: `swf.export` (SVG, TTF, sound, binary), PNGs rendered by the player
thread (transparent, scale, every frame of a sprite with one area). Each editor view is a place of
the history (`Place::Package { view }`, `swf::Node::key`), so Back walks out of clips. MCP
`studio_swf` (`tool.rs` + `mcp::swf_call`): the Electron tool's actions and arguments (movies, open,
status, tree, timeline, info, script, setScript, edit, remove, revert, text, layers, apply, discard,
addImage, replaceImage, exportImage, character, shapeStyle, shapeSvg, editText, staticText,
addGlyphs, export, compile), run in phases: `studio_swf_open` selects the movie, `studio_swf_act`
goes and acts, then once nothing is in flight the answer is read (`studio_swf_read`, a backend call,
or a PNG rendered on the MCP thread); `object` matches with or without the package's name. Driver:
`movie open <node>|frame|select|enter|up|goto|play|layout|tool|move <depth> <x> <y>|nudge|set <key>
<value>|remove|revert|hide|show|undo|apply|discard|zoom|fit|script
<n>|script-mode|check|stage|edit-script|color <id> <key> <hex>|svg <id> <file>|glyphs <id> <file>
<chars>|edittext <id> <prop> <json>|add-image <file>|export-png <scale> <out>|export-svg <out>`;
`studio_state.movie` (node, trail, placements, edits, undo, script view, stage, `onScreen`: the
selection's centre, handles and knob in window points). A moved or scaled object floats on a layer
of its own (`Stage::float`: the frame rendered once without it, then only the layer moves);
rotations, skews and mirrors re-render the frame instead (about half a second; gpui draws images
without a rotation). Pictures are cached by request key (`Stage::cache`, 24), and resting on a tree
row for 80 ms or selecting a clip on the stage renders that view ahead (`prerender`), so opening it
shows it at once. Pane sizes (tree, properties, timeline) are kept in the settings (`movie_panes`).
Driver probes `mv-tree` and `mv-tree-<node key>`; a missing probe's error lists the probes of its
family on screen. Test runs must not share the user's MCP port or settings: run the app with its own
`HOME` and `TEMPEST_STUDIO_MCP_PORT`; a window behind others does not repaint (bring it to the front
before screenshots).
