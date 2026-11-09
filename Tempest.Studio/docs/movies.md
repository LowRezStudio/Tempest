# Studio movie editor

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- **Movies** (`swf.rs` + `swf/{dict,display,preview}.rs`, `swf.info` / `swf.tree` / `swf.timeline` /
  `swf.preview` / `swf.bytes` / `swf.player` / `swf.libraries` / `swf.script` / `swf.compile` /
  `swf.apply`, `SwfPreview.svelte` + `components/swf/` on SwfMovie exports, state in
  `src/lib/studio/swf.svelte.ts`): an FFDec-style editor. The resource tree lists the movie, the
  root frames, every character by kind (`swf.tree`: the dictionary with linkage names, bounds, what
  each uses and is used by, attached tags) and the tag views; it opens on the root frames; the stage
  previews a character or a timeline frame with Ruffle (`swf.preview`: a movie cut to the target's
  bounds, or a fixed `rect`, holding its dependency closure, without the `exclude`d depths;
  **Layout** shows what the placements say, no scripts and nested sprites on frame 1, **Play** runs
  the sprite) with an overlay of every object's bounds and the movie's stage outlined; the timeline
  under it is the display list per frame (`swf.timeline`: depth, name, character, transform, the
  tags that placed and moved it). Double-click (or Enter) on a clip edits inside it, Backspace goes
  up, a breadcrumb (`Root › UIHome › …`) shows the path and the tree follows; **Focus** hides the
  package and export panes. The stage is an unbounded canvas (no scrollbars; middle-drag, Ctrl+drag
  or Space+drag pans, the wheel scrolls, Ctrl+wheel zooms at the pointer, Fit recentres:
  `SwfDoc.pan` / `fit`). Every editor view (root, clip, tag view) is a place of the window's
  Back/Forward history (`PackagePlace.view`, recorded by `SwfPreview`, opened by the `view` handler
  `swf.svelte.ts` registers), so Alt+← walks back out of clips. Ctrl+Z takes back the last pending
  edit first (`SwfDoc.undoEdit`, quick changes of one edit are one step; the stack is dropped when
  Apply writes the edits, whose package undo takes over), then the package's undo. Objects are
  selected on the stage (smallest box under the pointer, Alt+click cycles) or in the list, dragged
  or nudged with the arrows (Shift: 10 px), scaled and rotated with the free-transform tool (Q; V
  moves) and edited in the properties pane (name, x/y, scale, rotation, skew, alpha, hide, remove).
  The list's eye column hides layers from the stage render only (`SwfDoc.hiddenBySprite`, sent as
  `exclude`; Alt+click an eye or Solo selected shows one layer alone; Hide off-stage / Hide all /
  Show all), never written to the movie; layers drawn over nearly the same rectangle (overlap ≥ 75
  %, e.g. Home's four GoTo* panels) are badged "stacked ×N", since hiding one of them changes
  nothing visible, and the stage shows "updating view…" while a new render loads. Layout layers use
  Ruffle's 2D canvas renderer and pause once their picture has settled, since a playing layer costs
  about a third of a core; they swap in only then, so edits never show a half-loaded frame. Settled
  (`settled` in `swf-render.ts`): Ruffle has drawn something (its `setTransform` calls on the
  canvas's context are counted; while a large movie is still being parsed, an 8 MB one for 0.7 s,
  its frames are only cleared, and every layout preview carries a transparent 1 px shape above all
  layers so an empty layout draws too), then the thumbnail is unchanged for 60 ms and no shared
  library was fetched for 150 ms (`libraryFetchedAt` in `ruffle.ts`), or for one frame when the
  preview imports nothing; a picture not seen settled within 6 s keeps playing and is never cached.
  Load speed: layout previews are written at 255 fps (Ruffle draws a movie's first picture about ten
  frames after loading it: 400 ms at the UI's 24 fps), import only the libraries their characters
  use, with one `ImportAssets2` per URL (`swf::merge_imports`; Ruffle loads a library again for
  every import tag), and movies/libraries go to Ruffle uncompressed; `swf.preview` reports how many
  `imports` it kept. A layout only defines what nested sprites show on their first frame
  (`first_frame_closure`: 8.2 → 2.6 MB for endofmatchlobby's `UIEndOfMatchLobby`) and brings its own
  cut of each shared library (`libraries` in the reply, `preview::library_subset`, built recursively
  by `swf_subsets` in `serve.rs` and cached in `swf_subset_cache`): only the imported symbols'
  first-frame closure, the library's images embedded from its own package (whole libraries were
  served without them), served at content-named URLs (`swf-lib/<hash>.swf`); play targets, the Movie
  player and the full `swf.libraries` keep whole libraries (`previewLibraries` in `swf-render.ts`
  picks). Ruffle routes are reference-counted (two layers can serve the same cut) and report when
  all of a movie's libraries were fetched (`librariesFetched`), which shortens the settle check's
  library wait from 150 to 40 ms. Pictures (`src/lib/studio/swf-render.ts`): a settled layer's
  picture is kept in memory (~100 MB, by request key) and on disk (IndexedDB `tempest-studio-swf`,
  PNG, keyed by a SHA-256 of the preview bytes + libraries + background, 1500 entries), and a layer
  shows either one without starting Ruffle unless it has grown past its resolution; `prerender`
  renders views ahead, one at a time, newest first: resting on a tree row (`prerenderView`), on a
  movie in the export list or pressing it (`prerenderMovie`, the press comes ~80 ms before the
  click), the next movie when walking the list with the arrows, and a clip selected on the stage.
  Stage requests are built by `stageRequest`/`stageKey`/`defaultView` in `swf.svelte.ts` so a
  pre-rendered view has the key the stage will ask for; the stage waits for a sprite's timeline
  before asking (`timelineReady`). Ruffle's wasm is warmed when a package with movies opens
  (`warmRuffle`), and a document fetches its libraries right after reading the movie. Another movie
  mounts a fresh editor (`{#key doc}` in `SwfPreview.svelte`) and another clip or character a fresh
  base layer (keyed by target kind and id in `SwfStage.svelte`), so the previous picture never shows
  while the new one renders; frames and edits of one target still swap in place. The resource tree
  (`SwfTree.svelte`) is a virtual list (fixed row heights, only the rows in view are rendered: 1428
  rows took ~110 ms to mount). Window-wide: the package and export rows use `content-visibility:
  auto` (a full layout with a 6000-export package took ~130 ms), nothing on the selection path reads
  layout synchronously (`SwfLayer` takes its width from a ResizeObserver; the export tree does not
  scroll to a row that was clicked), and the host joins a large `upk serve` reply's chunks once
  instead of per chunk. The stage is stacked Ruffle layers (`swf/SwfStage.svelte`,
  `SwfLayer.svelte`): an object gets a layer of its own when first edited (`SwfDoc.float`) and the
  frame is re-rendered once without it, so later moves only change a CSS transform; a layer only
  reloads when its request key changes, and the view rect is fixed per target. **Make a mod** (shown
  once the package has applied changes) opens the mod cooker with the package chosen as a delta
  (`cooker.propose`); edits stay pending in the renderer (every view takes them as `edits`) until
  **Apply** writes them as one undo step (`place` rewrites a PlaceObject2/3 up to ClipDepth and
  keeps filters/blend/clip actions as stored, `deletePlace` drops a placement tag). The GFx tag
  stream of a movie is parsed and rewritten byte-identically (`upk swf verify <pkg>`); the tag views
  list the `DefineEditText` fields (initial text editable), the ActionScript 2 scripts (**Scripts**
  view, `SwfScripts.svelte` / `SwfScriptEditor.svelte`: every class definition, frame script of the
  root and of sprites, clip event and button action, decompiled to ActionScript 2 with `class`
  syntax for the Flash compiler's class definitions, or shown as P-code; Check compiles
  (`swf.compile`), Stage keeps the text as a pending `script` / `pcode` edit that Apply writes; the
  compiler and decompiler are `swf/as2/` in tempest-assets, see the [backend movie
  reference](../../Tempest.UnrealScript/docs/movies.md)), the `DefineExternalImage2` references
  (file name → the package's Texture2D, with a link; renameable), the linkage (`ExportAssets`,
  `ImportAssets2`) and every tag. The **Movie** node plays the whole movie with Ruffle
  (`src/lib/studio/ruffle.ts`, `SwfPlayer.svelte`): the self-hosted player is imported from
  `$ruffle`, a Vite alias for `vendor/ruffle/` when it exists, else `@ruffle-rs/ruffle` from npm:
  `scripts/build-ruffle.sh` (`just studio-ruffle`) builds Ruffle v0.7.1 with `ruffle/*.patch`
  applied (ruffle-rs/ruffle#23681, `scale9Grid` / `DefineScalingGrid` 9-slice scaling, which the
  game's UI panels use) into the gitignored `vendor/ruffle/`, and `studio-release.yml` builds it
  once in a `ruffle` job (cached on the patches and script) for every platform; restart the dev
  server after building it (`src/lib/studio/ruffle.ts` imports `ruffle.js` with `?url` and globs its
  core chunks and wasm as URLs so a build emits them, under their own names because the bundle
  builds its chunk/wasm URLs from them — see `environments.client` in `vite.config.ts`), and fetches
  only when a movie is opened. Ruffle resolves the movie's `ImportAssets2` URLs (shared UI
  libraries, fonts, image stacks) against the page, so the renderer serves the workspace copy of
  each by intercepting `fetch`; the backend finds them by export name (`swf.libraries`), and a URL
  no package provides is returned in `missing` and answered by the player with an empty movie
  (Ruffle drops the whole stage on a failed import — alpha HUDs import a `gfximagestack` they do not
  ship). `swf.player` embeds each `DefineExternalImage2` / `DefineSubImage` as a standard
  `DefineBitsLossless2` bitmap decoded from the package's `Texture2D` (crops honour the atlas rects
  and DXT block padding) before Ruffle sees it, so images draw without the game engine; textures
  whose mips are not reachable (streamed `.tfc` data with an unreadable `MergedFileIndexCache.bin`)
  are listed as missing by the player. Apply writes the movie back into the package (undoable).
  **Images**: an external image (properties pane, the External images view) shows its texture with
  Replace… (PNG/DDS through `texture.import` with `keepAlpha`: a translucent PNG turns a DXT1
  texture into DXT5; with `padBlocks` a size that is not a multiple of 4 is padded on the right and
  bottom like GFx's own images (D3D9 refuses such DXT textures: a fatal `CreateTexture ...
  D3DERR_INVALIDCALL` in game; without it the PNG is scaled to the nearest multiple of 4, unless the
  texture already has the padded size); the movie keeps drawing it at the tag's size, stretched when
  the aspect ratio differs, which the toast says) and Export PNG/DDS; renders carry the package
  stamp (`SwfDoc.stamp` = `studio.edits`) in their keys, so a replaced texture or an undo redraws.
  **Vectors, texts, fonts** (`swf/{shape,font,text,svg}.rs`, `swf.character` / `swf.export`,
  `SwfCharacterEditor.svelte`, `src/lib/studio/swf-export.ts`): the properties pane edits a shape's
  colours (solid fills, gradient stops, alpha on DefineShape3/4) and line widths in place
  (`shapeColor` edits patch the stored bytes) and replaces a shape with an SVG (`shapeSvg`: usvg
  resolves the SVG, each path becomes its own fill/line style of a DefineShape3, cubic curves split
  into quadratics, fill sides from contour direction and nesting; fitted into the old bounds by
  default); every edit-text property (font id or class, size, colour, align, margins, indent,
  leading, max length, variable, bounds, the flags; `editText`); static texts (DefineText/2: records
  decoded through the font's code table, re-encoded with the advances the movie's texts use, else
  the font layout, else the outline; a record's colour no longer leaks to the next; `staticText`); a
  font's characters (`fontGlyphs`: glyphs from a TTF/OTF via ttf-parser, inserted in code order with
  static texts and `DefineFontAlignZones` remapped; a font without a layout gets one). DefineFont2/3
  rewrite byte-identically (all 21 fonts of GFxFonts). Exports: SVG of a character, sprite frame or
  root frame (shapes, nested sprites at frame 1, static texts as outlines, masks, colour transforms,
  bitmap fills as patterns from the textures; edit-texts, buttons, morphs and hidden objects left
  out), PNG at 1/2/4× and every frame of a timeline as numbered PNGs (rendered by Ruffle in a hidden
  canvas player, `renderPng`), fonts as TrueType, sounds as MP3/WAV (ADPCM decoded), binary data.
  The toolbar's Export menu acts on what the stage shows. **Add image** (toolbar Image, Images view;
  `swf.addImage`, `swf/add.rs`) writes the pending edits, then duplicates a texture the movie
  already uses (same group, NetIndex -1) into a new uniquely named Texture2D from the PNG/DDS,
  appends it to the movie's `References`, inserts a `DefineExternalImage2` (id namespace, format
  code and extension copied from the movie's first image) and by default a `DefineShape` drawing it
  (clipped bitmap fill, 20 twips per pixel, top-left at the origin) after the opening tags, and
  optionally a `PlaceObject2` on the open timeline frame above every depth and mask; one undo step,
  rolled back if a step fails.
