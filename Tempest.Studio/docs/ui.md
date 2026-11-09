# Studio shell and interaction guidelines

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- **Shell** (`src/routes/+page.svelte`, `src/lib/components/shell/`): header (`AppHeader`: workspace
  menu with recent folders and workspace files, Back/Forward, the palette field, Undo, unsaved
  count, Save split button), activity bar (`ActivityBar`: Packages, Assembly, Maps, Mod cooker
  views, Search, Wwise, Tools menu, MCP, shortcuts), tab bar (`TabBar`,
  `src/lib/studio/tabs.svelte.ts`: each tab keeps a place; moving changes the active tab,
  Ctrl/middle-click on a package, export, table, reference or usage opens a new tab,
  Ctrl+W/Ctrl+T/Ctrl+Tab), status bar. Views are built from `Pane` (resizable by drag or arrow keys,
  double-click resets, sizes remembered per pane id in `ui.panes`; Ctrl+B hides the side pane,
  Ctrl+J the second one). `ui.compact` (window under 1100 CSS px) folds the side bar away and turns
  side panes into overlays. UI zoom (Ctrl+=/−/0, `window:zoom`, persisted). Every action is a
  command (`src/lib/studio/commands.svelte.ts` registry, `src/lib/studio/actions.ts` definitions
  with title, group, shortcut, enabled state): the window key handler, the command palette
  (`CommandPalette`: Ctrl+K / Ctrl+Shift+P commands with `>`, Ctrl+P packages and assembly tables,
  Ctrl+Shift+O objects of the open package or the row filter with `@`, Ctrl+Enter opens in a new
  tab), the menus (`Menu` takes command ids) and the shortcut list (F1, `ShortcutsDialog`) read from
  it. Dialogs open through `ui.open(id)`, one at a time. **Context menus** (right-click, or the menu
  key / Shift+F10 under the focused row): `ui.openContextMenu(event, items)` shows
  `shell/ContextMenu.svelte` (fixed, kept inside the window, arrow keys, closes on Escape, outside
  click, scroll, resize); items are the same `MenuItem`s as the dropdowns (`shell/MenuItems.svelte`:
  command ids, `-`, headings, ad-hoc items with icon, shortcut hint and `danger`), built in
  `src/lib/studio/contextmenus.ts` for packages, exports, tabs and recent entries, inline for
  assembly rows and tables, search results and the cooker's files (its `…` menu). Export actions run
  through `ui.objectAction`, which the inspector carries out once it shows the export. The
  screenshot harness takes `STUDIO_SMOKE_SIZE=WxH` (a window larger than the screen stops
  repainting: zoom out instead); dev builds expose `studio`, `assembly`, `rpc`, `session`, `ui`,
  `tabs`, `commands`, `nav`, `cooker`, `map` on `window`.

- **Keyboard**: Up/Down move through the package list (opening the package), the export tree (Right
  expands or enters, Left collapses or goes to the parent, Enter toggles, Home/End), the assembly
  table list and the assembly grid. Handlers live on each pane's container (`src/lib/studio/keys.ts`
  helpers) and ignore keys typed into fields.

- **Back / Forward** (header, Alt+←/→, mouse side buttons): one history per window for package
  selections and assembly rows alike (`src/lib/studio/nav.svelte.ts`; the stores register how to
  visit their places), so following a reference from a row into a package and back works.
