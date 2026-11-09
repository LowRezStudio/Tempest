# Native Studio: ui

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Shell and commands

Shell (`src/studio.rs`): header (Back/Forward, game folder menu with recent folders, search field
opening the palette, Undo, unsaved count with a list, Save menu: save, save all, export a copy,
discard), activity bar (Packages / Assembly / Audio / Map / Mod views, search, shortcuts), window
tabs (`tabs.rs`), status line, notifications (`Studio::toast`), welcome screen (`welcome.rs`). Every
action is a `Cmd` (`commands.rs`: title, group, keys, `enabled`, `run`): key bindings, the palette's
`>` mode, menus and the F1 shortcut list read it. The palette (`palette.rs`) is also the picker of
every "choose an object" question (`Pick`: copy to, replace with, slot material, texture parameter,
instance parent, copy from, template), searched with `workspace.search {isA}`.

## Settings

**Settings** (⌘,, `settings.rs`, `prefs.rs` → `<config>/tempest-studio-native/settings.json`): theme
dark/light/system, ask before saving in place (the save dialog also has Don't ask again), reopen the
last package/object/view per game folder, forget recent folders, unpin. Per folder the Studio
remembers pinned packages (pinned to the top, context menu), recently opened packages (listed first
in ⌘P) and the last place. Package context menu: Pin, Copy the file path, Show in Finder; export
menu and ⌘⇧C: Copy the object path (`Tables::object_path`); the package list's **Unsaved** switch
shows only edited packages.

## Theme

**Theme**: Catppuccin Mocha/Latte like the Electron apps (`themes/catppuccin.json`, loaded into the
gpui-component registry by `prefs::theme_mode`), text size ⌘=/⌘−/⌘0 (`prefs.zoom` → the theme's font
size, the root's rem).

## Multiple windows

**Windows**: New window (⌘⇧N) opens another window with its own backend (`studio::open_window`);
files opened from the OS (`on_open_urls`) go to the active window (`Studio::open_path`: `.twrk`,
`.upk` with its folder, or a folder), else a new one.

## Pane layout

**Panes** (`studio/layout.rs`, one `layout::Shared` per window read by the Packages, Assembly, Audio
and Map views): ⌘B folds the side pane away (package list, assembly files, `.pck` list, the map's
levels), ⌘J the second one (object list, row pane, events, the map's inspector); under 1100 pt the
window is compact: the side pane stays folded and ⌘B shows it over the view (a click beside it, or
opening a package or table, closes it); dragged widths are kept per group in `prefs.panes` (`main`,
`assembly`, `audio`; the map keeps `map_panes`). The Assembly grid pins the id and name columns
(`Assembly::pinned`, as in Electron: drawn over the scrolled cells at the sideways offset, never
hidden by the column chooser). The Mod view asks which game file an added file with an unknown name
replaces, or ships it as a new file.
