# Native Studio: assembly

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Assembly editor

Assembly view (`assembly.rs` the entity, model and edits; `assembly/side.rs`, `grid.rs`, `row.rs`,
`content.rs`), the Electron assembly editor at parity:

- **Side**: every file (`assembly.dat`, `Lang_<EXT>.dat`) with its marshal function, unsaved dot,
  every header field (click to edit; `GAME_VERSION_RAW` as major.minor.build.rev), tables filtered by
  name; a table's menu (Open, new tab, copy name, show file), ⌘/middle-click opens a tab, ↑ ↓ in the
  list open the next table (`tables_focus`). The first table opens with the files.
- **Grid**: rows read 500 at a time as they scroll into view (`rows` by position, `positions` by
  index); a column is as wide as its widest cell read so far, measured in characters of the mono
  font (`measure`/`layout`; pinned id and name columns 7 and 13 rem, 28 rem at most); only the rows
  and columns in view are laid out (`Layout::window`, unit-tested). Cells: `*_FLAG` y/n as switches,
  row sets as links, `*_ID` values with a link icon (⌘-click: new tab), `*_MSG_ID`/`*_VALUE_ID` with
  their text dimmed, missing cells dimmed. Cell cursor = selected row + `col`: arrows, Tab, Home/End,
  Page Up/Down (pages are read as needed, `select_pos`/`want_pos`), Enter/F2 edits or opens a set,
  typing starts an edit, Space flips a flag, Escape drops the marks, Delete asks. ⌘/⇧-click, ⇧↑↓ and
  ⌘A mark rows: an edit of a marked row sets them all (`mcts.setMany`), with a bar (Copy, Copy as
  TSV, Clear). ⌘C copies rows as JSON, ⌘V appends them with fresh ids, or pastes plain text into the
  active cell. Row menu (right-click): Edit, Pick a value… (`*_VALUE_ID`: the palette lists the valid
  values, `Pick::AssemblyValue`), copy value, go to the row, copy/TSV/paste, Duplicate, New row, new
  tab, Delete. Filter (applied after 250 ms), sort by a header click, column chooser with Show all
  (`assembly-columns.json`), breadcrumbs for row sets.
- **Editing** (`Edit`: Cell, Field, Header, Message): one single-line and one multi-line editor;
  Enter applies (a cell's cursor goes down), Tab applies and moves sideways, Escape cancels, leaving
  the field applies; ⌘Enter applies in the multi-line editor (messages, strings over 60 characters).
  The keys are bound in `AsmEdit > Input` over the input's own. An unchanged text writes nothing.
- **Row pane**: every field (kind in the tooltip), flags as switches, row sets and `*_ID` links,
  valid value names, Pick…, each message in every language file (edited, added where missing,
  opened in its `Lang_*.dat`), **Used by** on demand (`mcts.usages`: Find, then rows by path, field
  and label; a click goes to the nested row, ⌘-click a new tab).
- Toolbar: New content, New row, Duplicate, Delete, columns, ⋯ (copy/paste, Skins, Champion vendor,
  Export a mod, Export copies of the changed files, Reload, Undo with its label, Discard), Save….
  Commands (`Cmd::Asm*`, and Duplicate/Delete/Discard/Export a copy act on the grid in this view);
  ⌘P lists the tables, ⌘⇧O filters the rows of the table on screen.
- Places: `Place::Assembly { file, path, table, row }` (nested sets are history places too).
- The view asks the Studio through `with_studio` (deferred): the Studio may be the one updating it.
- Undoing every edit leaves a file clean (`Doc::clean_at` in `mcts/doc.rs`).
- Driver: `assembly table|row|col|mark|filter|sort|set|edit|message|header|open|up|follow|usages|
  usage <n>|add|duplicate|delete|undo|copy|tsv|paste|hide|show-all|reload|dump` (`dump` prints
  `state_json` with the selected row's cells); probes `asm-grid`, `asm-row`, `table-<name>`;
  `studio_state.assembly` is the same state.

New content is the Create wizard (`wizard.rs`, content kinds): champion and template by name, then
name, short description, description, slot (picked from the valid values or typed), a skin's part
names (from `mcts.collection`, following the skin's name until edited, `rename_part`), and the id
range for new content (`prefs.id_range`, applied with `mcts.idRange` when the assembly opens, kept
in workspace files, next ids shown from `mcts.nextIds`). Champion vendor (`mcts.vendor`/
`vendorAdd`).

## Skins and scaffolding

Assembly also has **Skins…** (`content.rs`: champions and their collections with parts, Open, Make a
copy… (the wizard at the skin's details, `Start::preset`), Put a loose skin in a new collection) and **Export mod…**
(`mcts.scaffold` into a chosen folder).
