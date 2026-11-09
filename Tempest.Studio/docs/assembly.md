# Studio assembly editor

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- **Assembly** (toolbar mode switch next to Packages; `src/lib/studio/assembly.svelte.ts`,
  `AssemblyView/Tables/Grid/Record.svelte`): edits the game's content database `assembly.dat` and
  the `Localization/<EXT>/Lang_<EXT>.dat` message files, MCTS marshals
  (`crates/tempest-assets/src/mcts/`, see the [backend assembly
  reference](../../Tempest.UnrealScript/docs/assembly.md)). The left pane lists every file with its
  header fields (DB_VERSION, GAME_VERSION_RAW shown and edited as `major.minor.build.rev`) and
  tables; the grid pages 500 rows (only the rows and columns in view are in the DOM; pages,
  summaries and messages are `$state.raw`, edits apply the status `mcts.set`/`setMany`/`deleteMany`
  return instead of refetching the document) with a substring filter over every cell, column
  sorting, double-click cell editing, nested row sets opened in place (breadcrumbs), `*_ID` columns
  as links to the row they reference (`refOf` in the store: the key column of each top-level table
  by name), `*_VALUE_ID` and `*_MSG_ID` values annotated with the valid value / INT message text;
  the row pane edits every field (flags as toggles, long strings as text areas) and shows each
  `*_MSG_ID` message in every language, editable in place (`mcts.setMessage`). **New row**
  (Ctrl+Shift+Enter) appends an empty row with the last row's shape and **Duplicate** (Ctrl+D)
  appends a copy of the selected row (`mcts.addRow`): rows always go at the end; the set's id column
  (named after the table: `DATA_SET_DEVICE_MODES` → `DEVICE_MODE_ID`; items, devices and skins share
  one id space) gets a fresh id from the id range, nested row sets are renumbered (rows that named
  the old id name the new one, their own id columns get fresh ids) and the copy's `*_MSG_ID`s get
  new messages with the same text in every language. The grid has a cell cursor (arrows, Tab;
  Enter/F2 or typing edits, Enter commits and moves down, Tab right, Esc cancels; Space toggles a
  flag), `*_VALUE_ID` editors suggest the valid values, flags are toggles, the id and name columns
  stay pinned, a column chooser hides columns per table (remembered); Ctrl/Shift+click and
  Shift+arrows mark rows (an edit of a marked row sets every marked row, `assembly.setMany`), Ctrl+C
  copies the rows as JSON (Ctrl+V into the same table appends them with fresh ids; plain text pastes
  into the active cell), "Copy as TSV", Delete twice deletes. The row pane's **Used by** lists every
  row of the file referring to its id (`mcts.usages`: same id family, `REF_BOT_ID`,
  `SPAWN_BOT_ID`…). Delete. Undo (Ctrl+Z), Discard and Save (Ctrl+S, in place) act on the `.dat`
  files when the mode is Assembly; Save as… writes the changed files into a folder. Back/Forward and
  the mouse side buttons walk the rows visited. The backend keeps the open files across a renderer
  reload (`mcts.open` reuses them unless `reload`). **New content…** (grid header; `mcts.create`,
  `mcts/create.rs`) clones a template row with fresh ids (`mcts.nextIds`; from the user's **id
  range** when one is set in the dialog, `studio.idRange` → `mcts.idRange`, so two mods made with
  the Studio never allocate the same ids) and new name/description messages in every language: a
  device or card/talent (devices + items rows, `REF_BOT_ID` and slot of your choice, nested mode ids
  reallocated), an item (and its skin row), a champion (bot row, one device per slot, Character
  Class and Weapon items, default skin, all on the new bot, plus an entry for the Character Class
  item in loot table 475 "Champion Vendor": the client builds its champion list (lobby and champion
  select, offline too) from that table's active entries, not from the bots, so a champion without an
  entry is never listed), or a skin (`kind: "skin"`, `mcts/skins.rs`: the template is a collection
  of loot table 1076 "Inventory Vendor - Collections", which the 8.1 client groups per champion
  (`UIDataChampions.m_PerChampionCollections`) and resolves to the skin through the entry's sub loot
  table; the copy is the container item (type 1634), every part (skin 10230, head 15170, weapon
  15041, voice, entitlement 11403 with its `REF_ITEM_ID` and prop 2143 pointed at the new container
  and entry) with its skin row, a new sub loot table (type 1455) and the 1076 entry; part names swap
  the template's theme word and champion for the new ones, editable per part (`names`)). **Skins…**
  (grid header; `mcts.collections` / `mcts.collection` / `mcts.collectionAdd`,
  `SkinCollectionsDialog.svelte`) lists a champion's collections with their parts, opens one in the
  grid, starts a copy (`ui.newContent` presets New content), and wraps a skin no collection holds in
  a new collection. Dialog edits refresh every document's state (`assembly.touched()`). **Champion
  Vendor…** (grid header; `mcts.vendor` / `mcts.vendorAdd`, `ChampionVendorDialog.svelte`) shows the
  vendor's active/total count against the 60 in-match buttons and lists the champion bots whose
  Character Class item has no entry, with an Add button that appends one (clone of an active entry,
  fresh `LOOT_TABLE_ITEM_ID`, next sort order) and opens it in the grid. **Export mod…**
  (`mcts.scaffold`) writes a launcher mod folder: `manifest.toml`, the assembly and changed language
  files as row deltas under `deltas/ChaosGame/…` when the Studio recorded their base (else whole
  under `files/ChaosGame/…`), `paserver.json` (the `GET_PROFILE_CLASSES` row and inventory items the
  private server must answer for a champion, `mcts.serverRows`), `scripts/main.lua` with
  `tempest.native` handlers and `src/<Pkg>/Classes/TgDevice_<Name>.uc` stubs (`dllimport` functions)
  for the chosen devices, and a README with the build steps.
