# Native Studio: ui

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Shell and commands

Shell (`src/studio.rs`): header (Back/Forward, game folder menu with recent folders, the **mod chip**
(`project.rs` `render_project_chip`: the named mod with its save state and menu, else "Unnamed mod ·
n changes" with Name this mod…/Open a mod…/Discard the draft…, else "No mod yet"), Create, the
search field opening the palette, Undo, Redo, **Changes (n)** (the changed files: open, discard,
advanced "Patch the game…"), **Build mod** (the one primary action, opens the Mod view); three
columns, so Create and search stay centred at normal widths while left/right shrink safely when
compact or zoomed, and what comes and goes sits at the far right), 60 pt labeled activity bar
(`Start`, `Champs` ⌘1, `Files` ⌘2, `Data` ⌘3, `Sounds` ⌘4, `Maps` ⌘5, then `Mod` ⌘6 as icon + 10 px
short label tiles; search, shortcuts and settings at the bottom; the views are disabled until a game
folder is open; OS minimum window size 960×640 pt), window tabs (`tabs.rs`), status line (shows
quiet view-appropriate guidance when switching views and Undo/Redo boundary notices), notifications
(`Studio::toast`; error toasts get a what-to-do sentence from `ui::explain_error` for locked/missing
files, wrong folders, a stopped engine, a full disk), the native menu bar (`menus.rs`: one menu per
command group, with the commands' shortcuts, an app menu with Settings, Check for updates and
**Quit**, which keeps every window's work first like an update restart).

**Where work lives**: every change is kept in "your mod". Without a project it is an unnamed draft
(the recovery `.twrk`, written about 2 s after edits settle, never over an earlier session's draft
still offered in the banner); **⌘S** saves the project or, without one, opens Name your mod (the
project dialog, which takes the draft's changes in). ⌘S never writes into the game folder: writing
in place is the advanced "Patch the game…" (`write_to_game`/`save_packages`, a danger confirmation
saying only the launcher's Verify game files puts originals back; Settings ▸ Ask before overwriting
game files). The UI says **changes** (not edits or unsaved), **game files** (packages), **game
data** (the assembly tables), **sounds**, **Build mod** (not cooker).

**First run** (`welcome.rs`, `games.rs`): the welcome screen lists the games found on the computer
(Tempest Launcher instances from `com.lowrezstudio.tempest/settings.json`, last played first, then
Steam libraries' Paladins), one Open each; a folder that fails to open shows its error inline.
After a folder opens the **Start page** (`home.rs`, `View::Home`) shows unless the last place is
restored: six task rows (champion look → Champions, stats and cards → Game data on `DATA_SET_DEVICES`
row 0, sounds → Sounds, maps → Maps, menus and HUD → the palette's curated Menus & HUD `@ui` scope,
something new → Create), a Your mod card, How it works in three steps, and the other mods.

**Champions** (`champions.rs`, `View::Champions`): the champion vendor's champions (`mcts.choices
{kind: bot}`), and for one: the champion's packages by prefix with their role (`package_role`)
first ("Look"), skin collections (`mcts.collections`) with their icons and abilities/cards/talents
(`mcts.choices {kind: device}`), lists capped at 8 with Show all. Icons: `mcts.icons` (item →
`ICON_ID` → `DATA_SET_ICONS.TEXTURE_RES_ID` → resource name, plus the bot's `DEFAULT_SKIN_ITEM_ID`
and its icon, the header portrait) read with `texture.preview {object, maxSize}` from
`<PREFIX>_UI_ASSETS_SF`. A skin unfolds its own files: `object.deps` of
`<group>.skinreferencer<part item>` in `<PREFIX>_ASSETS_SF` less those of
`<group>.botreferencer<bot>`, meshes/materials/textures/particles, each opening in Game files; its
game data row is a button on the row. The package
list names a champion's packages in a tooltip, and its filter ignores spaces ("bomb king").

Every action is a `Cmd` (`commands.rs`: title, group, keys, `enabled`, `run`): key bindings, the
palette's `>` mode, the menu bar and the F1 shortcut list read it. ⌘F (`FindInView`) focuses the
filter of the view on screen, ⌘⇧F searches every package; Undo is ⌘Z and Redo is ⌘⇧Z (Ctrl+Y
elsewhere) through `package.undo`/`package.redo`, `mcts.undo`/`mcts.redo`, Wwise `.pck` sound
replacements, a map's levels and a movie's pending edits. The palette (`palette.rs`, with `All`,
`Files & tables`, `Menus & HUD`, `Objects (#)` and `Commands (>)` scopes) is also the picker of
every "choose an object" question (`Pick`: copy to, replace with, slot material, texture parameter,
instance parent, copy from, template), searched with `workspace.search {isA}`.

**Look** (`ui.rs`): frame surfaces (header, activity bar, status line: `title_bar` = Catppuccin
crust), side panes (`sidebar`, mantle), work area (`background`, base), raised cards and inputs
(`secondary`). The accent is for one primary action per surface and the selection; toggles are
ghost buttons with `.selected`. Shared pieces: `pane_header` (32 pt min, sentence case), `toolbar`
(36 pt min), `row_h` (scales 24 pt list rows with text zoom), `badge` (20 pt min), `section`,
`hint`, `empty`/`loading`/`error` states, `kbd`, `mono()` (the platform's monospace). Muted text
(`#a5acc7` in Mocha, `#52556d` in Latte, kept at full opacity) and Latte's darkened primary
(`#3e50d8`) and category tones achieve ≥ 4.5:1 across every surface, including `surface0`,
`list.active` and `selection` (`#b4befe33` in Mocha).

## Settings

**Settings** (⌘,, `settings.rs`, `prefs.rs` → `<config>/tempest-studio-native/settings.json`): theme
dark/light/system, ask before saving in place (the save dialog also has Don't ask again), reopen the
last package/object/view per game folder, forget recent folders, unpin. Per folder the Studio
remembers pinned packages (pinned to the top, context menu), recently opened packages (listed first
in ⌘P) and the last place. Package context menu: Pin, Copy the file path, Show in Finder; export
menu and ⌘⇧C: Copy the object path (`Tables::object_path`); the package list's **Edited** switch
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
levels), ⌘J the second one (object list, row pane, events, the map's inspector); under 1120 pt the
window is compact: the side pane stays folded and ⌘B shows it over the view (a click beside it, or
opening a package or table, closes it); dragged widths are kept per group in `prefs.panes` (`main`,
`assembly`, `audio`; the map keeps `map_panes`). The Assembly grid pins the id and name columns
(`Assembly::pinned`, as in Electron: drawn over the scrolled cells at the sideways offset, never
hidden by the column chooser). The Mod view asks which game file an added file with an unknown name
replaces, or ships it as a new file.

Commands also cover: Open package file (⌘O), Export a copy (⌘⇧S), Search every package (⌘⇧F),
Next/Previous tab, Zoom in/out/reset, Toggle theme, Check for updates, MCP server, Package info,
Rewrite, Prepare a mod folder (advanced), Export the class sources, Show the mod's folder, Discard
all changes, and the game data's (`Asm*`). The palette lists the assembly's tables
with the packages, and `@` in the Assembly view filters the rows of the table on screen.
