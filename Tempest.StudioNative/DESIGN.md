---
name: Tempest Studio (native)
description: A Paladins mod editor a player can use on day one; Catppuccin Mocha/Latte with a stricter editor system on top.
colors:
  lavender: "#b4befe"
  crust: "#11111b"
  mantle: "#181825"
  base: "#1e1e2e"
  surface0: "#313244"
  surface1: "#45475a"
  text: "#cdd6f4"
  subtext0: "#a6adc8"
  overlay2: "#9399b2"
  tone-mauve: "#cba6f7"
  tone-yellow: "#f9e2af"
  tone-green: "#a6e3a1"
  tone-blue: "#89b4fa"
  tone-sky: "#89dceb"
  tone-red: "#f38ba8"
typography:
  headline:
    fontFamily: "system-ui (GPUI default UI face)"
    fontSize: "20px"
    fontWeight: 700
  title-lg:
    fontFamily: "system-ui"
    fontSize: "18px"
    fontWeight: 700
  title:
    fontFamily: "system-ui"
    fontSize: "16px"
    fontWeight: 600
  pane-title:
    fontFamily: "system-ui"
    fontSize: "14px"
    fontWeight: 600
  body:
    fontFamily: "system-ui"
    fontSize: "14px"
    fontWeight: 400
  label:
    fontFamily: "system-ui"
    fontSize: "12px"
    fontWeight: 600
  meta:
    fontFamily: "system-ui"
    fontSize: "12px"
    fontWeight: 400
  mono:
    fontFamily: "Menlo, Consolas, DejaVu Sans Mono"
    fontSize: "12px"
    fontWeight: 400
rounded:
  sm: "2px"
  md: "6px"
  lg: "8px"
  full: "9999px"
spacing:
  "1": "4px"
  "2": "8px"
  "3": "12px"
  "4": "16px"
  "6": "24px"
  "8": "32px"
  "10": "40px"
components:
  button-primary:
    backgroundColor: "{colors.lavender}"
    textColor: "{colors.crust}"
    rounded: "{rounded.md}"
  button-ghost:
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
  button-secondary:
    backgroundColor: "{colors.surface0}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
  header:
    backgroundColor: "{colors.crust}"
    height: "40px"
  activity-bar:
    backgroundColor: "{colors.crust}"
    width: "44px"
  status-line:
    backgroundColor: "{colors.crust}"
    textColor: "{colors.overlay2}"
    typography: "{typography.meta}"
    height: "24px"
  pane-header:
    textColor: "{colors.text}"
    typography: "{typography.pane-title}"
    height: "32px"
    padding: "0 8px"
  toolbar:
    backgroundColor: "{colors.base}"
    height: "36px"
    padding: "0 8px"
  search-field:
    backgroundColor: "{colors.surface0}"
    textColor: "{colors.overlay2}"
    rounded: "{rounded.md}"
    width: "360px"
    height: "26px"
  list-row:
    textColor: "{colors.text}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "4px 8px"
  task-row:
    rounded: "{rounded.lg}"
    padding: "10px 12px"
  card:
    backgroundColor: "{colors.mantle}"
    rounded: "{rounded.lg}"
    padding: "16px"
  kbd:
    backgroundColor: "{colors.surface0}"
    textColor: "{colors.overlay2}"
    typography: "{typography.meta}"
    rounded: "{rounded.sm}"
    padding: "0 4px"
---

# Design System: Tempest Studio (native)

## Overview

**Creative North Star: "The Player's Workbench"**

A quiet, dark editor shell in Catppuccin Mocha (Latte in light mode) where the player's words come first and the engine's second. Structure carries hierarchy: four surface levels from the crust frame to raised surface0 cards, one lavender accent per surface, and Lucide line icons. Newcomer surfaces (Start, Champions, Mod) are roomy; power surfaces (Game files, Game data, Map) stay dense and keyboard-first, with every action also a `Cmd` in `commands.rs`.

Tokens live in `themes/catppuccin.json` (gpui-component theme fields); code reads them as `cx.theme().<field>`. Never hard-code a hex in Rust: pick the field. The frontmatter lists Mocha values; Latte equivalents are in `.impeccable/design.json` (`colorMeta.*.latte`).

**Key Characteristics:**
- Flat, tonal layering; no shadows (`"shadow": false`).
- One primary (lavender) button per surface; selection is the only other accent.
- Sentence-case titles, 14px rows, 12px meta and hints.
- Plain-word labels with the technical name second, in mono and muted.

## Colors

Catppuccin Mocha/Latte, modified only where the editor needed a clearer frame and more legible muted text.

### Primary
- **Lavender** (`primary`, `ring`, `list.active.border`, `selection` at 30%, `list.active` at 20%): the one main action of a surface (Build mod in the header, Build & install in the Mod bar, Open on the welcome screen's first game) and the selection. Text on it is crust (`primary.foreground`).

### Tertiary (category tones)
- **Mauve, Yellow, Green, Blue, Sky, Red** (`theme.magenta`, `yellow`, `green`, `blue`, `cyan`, `red`): only as the six task icon plates on the start page (icon in the tone, plate at 14% of it). Red doubles as `danger`, yellow as `warning`, green as `success`; those three also mark state (problems, the mod chip's save dot).

### Neutral (surface ladder, darkest to lightest in Mocha)
- **Crust** (`title_bar.background`, `status_bar.background`): the frame: header, activity bar, status line. Modified from stock (both set to crust).
- **Mantle** (`sidebar.background`, `popover`, `list.head`): side panes (wrapped by `layout::group` for `Role::Side`), the Mod view bar, the "Your mod" card.
- **Base** (`background`): the work area and view toolbars.
- **Surface0** (`secondary`, `border`, `muted.background`): raised things (inputs, the search field, key caps) and every 1px rule.
- **Surface1** (`input.border`, `secondary.hover`): input strokes, hovered secondary buttons.
- **Text** (`foreground`): titles and rows.
- **Subtext0** (`tab.foreground`, `table.head.foreground`): inactive tabs, table heads.
- **Overlay2** (`muted.foreground`): hints, meta, section labels, toolbar icons, status line. Modified from stock for legibility (Latte uses subtext1).

### Named Rules
**The One Lavender Rule.** A surface has at most one `.primary()` button. When a prerequisite step exists (an unnamed mod), that step takes the primary and the main action drops to default (`cook-build` loses `.primary()` while "Name this mod…" holds it).

**The Tone Plate Rule.** Category tones appear only on the start page's task plates and as state color (danger/warning/success). They never fill buttons, headers or rows.

## Typography

**UI Font:** the platform's system UI face (GPUI default)
**Mono Font:** `ui::mono()`: Menlo (macOS), Consolas (Windows), DejaVu Sans Mono (Linux)

**Character:** Native and unadorned; weight and muted color do the hierarchy, not size jumps.

### Hierarchy
- **Headline** (700, 20px, `text_xl`): one per page, the start page question only.
- **Title large** (700, 18px, `text_lg`): the mod's name in the Mod view bar.
- **Title** (600, 16px, `text_base`): champion page sections (Skins, Abilities…), the "Your mod" card title.
- **Pane title** (600, 14px, `text_sm`): `ui::pane_header`; task row titles.
- **Body** (400, 14px, `text_sm`): list rows, search field, page lede.
- **Label** (600, 12px, `text_xs`, muted): `ui::section`; group labels like "Your mods", "How it works". Sentence case.
- **Meta** (400, 12px, `text_xs`, muted): `ui::hint`, row details, counts, status line.
- **Mono** (12-14px): ids, paths, code, raw table names, row counts.

### Named Rules
**The Sentence Case Rule.** Every title, label and button is sentence case. No uppercase, no tracking.

**The Plain Name First Rule.** Where an engine name exists, show the friendly name in body text, then the raw name in mono at muted 70% (`assembly/side.rs::friendly_table`). Unmapped names fall back to mono alone.

## Layout

Fixed shell at a 1440x900 default window: header (40px) over activity bar (44px) + view + status line (24px). Views compose resizable panes with `layout::group` (`Role::Side` / `Main` / `Second`); pane widths persist via `pane_sizes`. In compact windows the side pane becomes an overlay drawer over a scrim (`theme.overlay`).

Spacing is GPUI's 4px unit: 8px (`px_2`) pane and toolbar insets, 12px row padding, 16px card padding, 32/40px page margins on the start page. Pane headers are 32px (`ui::PANE_HEADER`), view toolbars 36px (`ui::TOOLBAR`) with a bottom rule. The start page centers a 340-560px task column and a 300px side column, wrapping, 40px apart.

Density follows the audience: newcomer pages breathe (task rows 10px tall padding, 40px gaps); Game files, Game data and Map keep power-user density.

## Elevation & Depth

Flat. Depth is the surface ladder (crust < mantle < base < surface0) plus 1px `border` rules; the theme sets `"shadow": false`. The single exception is the compact-mode side drawer (`shadow_lg` over the scrim), which floats over content by necessity.

### Named Rules
**The Ladder Rule.** A new region picks its rung by role (frame, side pane, work area, raised), never a new color. Need separation inside a rung? Use a 1px `border` rule.

## Shapes

Gently rounded: 6px (`radius`, `rounded_md`) for buttons, inputs, list rows and icon plates; 8px (`radius.lg`, `rounded_lg`) for cards, task rows and the Mod bar plate; 2px for key caps; full circles for status dots (7px) and step numbers (20px, bordered). No other silhouettes.

## Components

Helpers live in `src/studio/ui.rs`; use them before writing a new row of styles.

### Buttons (gpui-component `Button`)
- **Primary** (`.primary()`): lavender fill, crust text, Lucide icon + label ("Build mod", "Build & install"). One per surface.
- **Default** (no variant): surface0 fill, for secondary commands that stand on their own ("Create").
- **Ghost** (`.ghost()`): everything else: header tools, the mod chip, Mod bar actions, list actions. `.selected(bool)` marks the active activity-bar view and the active Mod tab.
- **Danger** (`.danger()`): destructive confirmations only ("Discard and close").
- **Sizes:** `.small()` in header and bars, `.xsmall()` inside panes. Disabled buttons carry a tooltip saying why.

### Pane header and toolbar
- `ui::pane_header(title, tools, theme)`: 32px, 14px semibold title that ellipsizes, muted tools on the right (counts, small ghost buttons).
- `ui::toolbar(theme)`: 36px min, base background, bottom rule.

### States (`ui.rs`)
- **Empty** `ui::empty(icon, title, body)`: large muted Lucide icon at 70%, 14px semibold title, 12px body capped at 340px, caller adds action buttons.
- **Loading** `ui::loading(text)`: small spinner + muted 14px sentence ("Reading the champions…").
- **Error** `ui::error(title, detail)`: `CircleAlert` in danger, title, detail capped at 420px; caller adds the way out. Pass backend text through `ui::explain_error` so common causes get a what-to-do sentence. Transient errors go to the toast path.

### Lists and rows
- **List row:** 14px, 4px/8px padding, 6px radius; hover `list_hover`, selected `list_active` (lavender 20%).
- **Link row** (champion page): title + muted detail + `ArrowUpRight`; opens where the thing is edited.
- **Task row** (start page): 36px tone plate, 14px semibold title, 12px muted detail, `ChevronRight`; whole row clickable, 8px radius.
- **Kbd** `ui::kbd(keys)`: key cap for shortcuts; always format keys via `commands::key_hint`.

### Navigation
- **Header** (crust, 40px): back/forward, game folder, mod chip, Create, search field (opens the palette, ⌘K), undo/redo, "Changes (n)" when n > 0, Build mod (primary, hidden in the Mod view).
- **Activity bar** (crust, 44px): Start, Champions, Game files, Game data, Sounds, Maps, rule, Build mod; then Search, Shortcuts, Settings at the bottom. Ghost icon buttons, `.selected` for the current view, tooltip "Name: what it holds (keys)".
- **Status line** (crust, 24px): 12px muted status text left, counts right.
- **Native menu bar** (`menus.rs`): generated from `Cmd::ALL` by group; the Assembly group is titled "Game data". New actions get a `Cmd`, never a menu-only item.

### Mod chip (`project.rs`)
A 7px dot + ghost `Briefcase` button with a dropdown. Dot: warning while changes are pending or saving, success when saved, danger on failure, muted 50% with no mod. Label "Unnamed mod · n changes" / "No mod yet" / "Name · state".

### Mod view bar (`cooker.rs`)
Mantle bar, 16px/12px padding: 40px `Hammer` plate, 18px bold mod name over a 12px status line (warning color when something blocks the build), then Remove from game, Play in Tempest Launcher, an `Ellipsis` menu (Build only, Install elsewhere, Save a copy, Publish), and Build & install (primary). Below: ghost `.selected` tabs Changes / Details / Publish.

## Do's and Don'ts

### Do:
- **Do** read colors from `cx.theme()` fields (`title_bar`, `sidebar`, `background`, `secondary`, `primary`, `muted_foreground`, `list_active`, `list_hover`, `border`) and add any new token to both themes in `themes/catppuccin.json`.
- **Do** use the `ui.rs` helpers for pane headers, toolbars, sections, hints, empty, loading and error states.
- **Do** use only Lucide icons registered in `src/assets.rs` (`icon_assets!`); register a new one there first.
- **Do** write UI copy in the player's words: "changes", "game files", "game data", "sounds", "Build mod", "your mod"; technical names second, in mono.
- **Do** give every disabled control a tooltip with the reason, and every error a way out.

### Don't:
- **Don't** put more than one `.primary()` button on a surface (The One Lavender Rule).
- **Don't** use category tones outside the start page plates and danger/warning/success state.
- **Don't** add shadows; separation is the surface ladder and 1px rules.
- **Don't** use "cooker", "unsaved" or "write into the game" in UI copy; element ids may keep them.
- **Don't** use uppercase labels, kickers or eyebrows above titles; section labels are sentence-case group names only.
- **Don't** use emoji or text glyphs as icons.

## Known gaps

- No champion portraits: the Champions list is names only (no icon lookup yet).
- Skin rows open the game data row, not the skin's package (no skin to package mapping).
- The Mod view lists changed files, not changed rows.
- The Map view keeps its own power-user density and does not follow the newcomer spacing.
- Focus rings show only on gpui-component inputs; custom clickable rows (task rows, list rows, the search field) have hover but no keyboard focus ring.
