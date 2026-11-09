# UnrealScript: movies

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Movies (`swf.rs`)

GFx/SWF container (`GFX`/`FWS` plain, `CFX`/`CWS` zlib; LZMA refused), header rect/rate/count kept
as bytes, tags with their original length form so an untouched movie rewrites byte-identically
(every movie of the 8.1 UI packages does). Parsed for the Studio: `DefineEditText` (flag bytes are a
big-endian bit field), `DoInitAction`/`DoAction` strings (constant pools, pushes), every script
through `swf/as2` (below), `DefineExternalImage2` (u32 id, format, size, export name, file name),
`ExportAssets`, `ImportAssets2`, sprites. Edits re-serialise only the touched tag; script string
edits are limited to top-level actions (relative branches are unaffected, `DefineFunction` bodies
are not touched).

`swf.bytes` returns the movie, `swf.player` returns it with every
`DefineExternalImage2`/`DefineSubImage` replaced by a `DefineBitsLossless2` bitmap decoded from the
package's `Texture2D` (the external image's 32-bit id keeps its low 16 bits as the character id;
atlas crops scale into the decoded texture and DXT block padding on the right/bottom is cropped),
and `swf.libraries` resolves its `ImportAssets2` URLs (Scaleform splits the UI into
`../SharedAssets/SharedAssets.swf`, `../GFxFonts/gfxfontlib.swf`, image stacks, …) by the URL's file
stem to a `SwfMovie` export of the workspace, returning those movies too; URLs no workspace package
provides come back in `missing`. The external image namespace is `0x0009_0000 | character id`;
textures whose mips are not reachable stay in `missing` and are left for Ruffle to ignore.

The editor's model (`swf/`): `dict.rs` builds the character dictionary (kinds, the defining tag,
dependencies from shape bitmap fills, sprite/button placements, text fonts, sub-image → atlas;
bounds from the tags' rects, sprites and buttons computed from their first frame; linkage names;
tags attached by id such as `DoInitAction`, scaling grids, font extras; imported symbols).
`display.rs` holds `MATRIX`/`CXFORM` codecs, `PlaceObject` 1/2/3 parse/write (fields up to
`ClipDepth` re-encoded, the rest kept as stored) and `timeline`, which plays a tag stream and
records per frame each depth's character, transform and the stream indices of the tags that placed,
moved and last changed it. `preview.rs` writes a standalone movie for one target (root frame, sprite
frame as a layout, or one character): definitions of the dependency closure, the imports among them
(every import when playing; one tag per URL, `merge_imports`), the target's list placed on the root
shifted to the origin, at 255 fps for layouts (Ruffle's first picture comes ~10 frames after load),
with a transparent 1 px `DefineShape3` (a free character id) placed above every layer of a layout so
the Studio can tell a drawn frame from a blank one. A layout's closure follows nested sprites into
their first frame only (`first_frame_closure`), and its imports point at subsets of the libraries
(`library_subset`: the imported names' first-frame closure, exports and the library's own imports
among them; `swf_subsets` in `serve.rs` builds them recursively, embeds each library's images from
its own package and names them `swf-lib/<hash>.swf`), returned as the reply's `libraries`.
`swf.player` and `swf.libraries` also merge import tags per URL and send movies uncompressed.
External images convert in parallel (`embed_images` → `ImageJob`s, `par::map`), and their
`DefineBitsLossless2` bitmaps use zlib's fast level (they only go to the Studio's Ruffle). `apply`
edits placements inside the root or any `DefineSprite` stream (`with_stream`). `shape.rs` parses
DefineShape 1–4 (styles with the byte offset of every colour and width, absolute edges, contours
chained per style, fill sides: fill0 left, fill1 right) and writes shapes and glyphs; `font.rs`
reads DefineFont/2/3 (rewrites 2/3 byte for byte), exports TrueType and adds glyphs from TTF/OTF;
`text.rs` reads and writes DefineEditText fields and DefineText records, exports sounds and binary
data; `svg.rs` exports SVG and imports it through usvg. `add.rs` builds new characters: a
`DefineExternalImage2` in the movie's id namespace/format, a `DefineShape` drawing a bitmap (clipped
fill, 20 twips per pixel), definitions inserted after the opening tags, and a `PlaceObject2` above
every depth and clip depth before a frame's `ShowFrame` (`swf.addImage` also creates the Texture2D
and lists it in the movie's `References`). `serve.rs` keeps the last parsed movie (`swf_cache`,
dropped by any call outside `swf.*` except a few reads such as `object.text` and `texture.preview`,
and replaced by `swf.apply` / `swf.addImage`) and the converted external images (`swf_embeds`, by
package and tag bytes), remembers where each imported library lives (`swf_libs`, by URL stem,
checked on use), and reads movie bytes through `Editor::with_object` without copying the parsed
export.

ActionScript 2 (`swf/as2/`): `mod.rs` finds every script of a movie (`Loc`:
`DoAction`/`DoInitAction` on the root or in a sprite's stream, the clip action records of
`PlaceObject2/3`, `DefineButton`/`DefineButton2` conditions; key `12`, `s40:3`, `s40:3e1`, `b7:0`)
and replaces one (`replace`, record and condition sizes rewritten). `action.rs` decodes an action
stream into `Action`s whose branches, `Try`/`With` sizes and `DefineFunction` bodies are instruction
indices (`Label`) and encodes it back byte for byte; `pcode.rs` is its text form (one action per
line, `L1:` labels, `c3:"…"` constants, `r1` registers, `1` int / `1.0` double / `1.5f` float) and
assembler. `ast.rs` is the syntax tree shared by `decompile.rs` (stack simulation; structures the
Flash compiler's shapes: `if`/`else`, `?:`, `&&`/`||` via `PushDuplicate`, `while`/`for`/`do`, `for
… in` on `Enumerate2` + r0 with its drain loops for `break`/`return`, `switch` on a stored value,
`try`, `with`, chained `a = b = v` through r0, unused call results the Flash compiler leaves on the
stack flushed as statements; registers named `_locN_`, renamed when a nested function reads that
name; a function it cannot structure stays P-code as `function (…) __pcode(regs, "flags", param
regs…) { … }`), `parse.rs` and `compile.rs` (one constant pool per script, `DefineFunction2` with
preloads and register parameters/locals except names nested functions read or `with`/`eval`
functions, `_locN_` keeps register N, `_loc1_`–`_loc3_` stay registers at the top of a script;
opcodes without AS2 spelling are `__cast(Type, x)`, `__extends(Sub, Base)`, `__implements`, …).
`class.rs`: `class`/`interface` syntax over the Flash compiler's `DoInitAction` class prologue
(package objects, constructor in `_loc1_`, prototype in `_loc2_`, members in order, `addProperty` in
name order, `ASSetPropFlags`); the decompiler only raises a class when `lower` gives back exactly
the statements it found. `upk as2 list|pcode|source|verify|recompile` works on extracted movies
(`upk swf dump <pkg> <dir>`); `verify` checks per script decode/encode and P-code byte identity,
full decompilation, and decompile → compile → decompile giving the same source, and counts
recompiled scripts whose operations equal the Flash compiler's (8.1 UI packages: 5111 scripts all
pass, 3600 recompile to the same operations; the user's FFDec-saved LobbyUI: 4209 pass). The
Studio's `swf.script` / `swf.compile` and the `script` / `pcode` edits of `swf.apply` use it.

Large byte arrays: a movie's `RawData` decodes to one `Value::Byte` per byte (7 M for the live
`Home`, 17.7 M for an FFDec-saved one). `json::value` sends at most 2000 elements of a byte array
(50 000 of any other) with the real `len`, since rendering each byte as a JSON object took the
Studio past 17 GB; `Editor::snapshot` keeps no parsed copy of a clean export over 1 MiB (undo
reparses it from the payload). Clone such objects only when the edit needs it.
