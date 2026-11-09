# Native Studio: assembly

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Assembly editor

Assembly view (`assembly.rs`, its own entity): files and tables, paged grid (filter, sort,
double-click edit, row sets in place, `*_MSG_ID`/`*_VALUE_ID` texts), row pane (fields, messages in
every language, Go to referenced rows, Used by), header version, New
row/Duplicate/Delete/Undo/Save/Discard, New content (`content.rs`: device/card/item/champion/skin
via `mcts.create`, optional id range `mcts.idRange`) and Champion vendor
(`mcts.vendor`/`vendorAdd`).

## Skins and scaffolding

Assembly also has **Skins…** (`content.rs`: champions and their collections with parts, Open, Make a
copy… presetting New content, Put a loose skin in a new collection) and **Export mod…**
(`mcts.scaffold` into a chosen folder).
