---
version: 1
slug: "tempest-studionative-src-studio-rs"
primary_target: "Tempest.StudioNative/src/studio.rs"
related_targets: ["Tempest.StudioNative/src/studio"]
---

# Tempest Studio Native: editor shell and views

Mode: Operate (desktop editor). Audience and jobs: Tempest.StudioNative/PRODUCT.md.
Build path: code-led (no image generation in this session); native GPUI app, no web detector.

## Direction contract

THESIS: A Paladins mod editor a player can use on day one. Every surface answers "what can I change, where does my change go, how do I get it into the game", in the player's words first and the engine's second. It refuses the category default of a raw asset browser (993 package names, table tokens, "cooker", "write into the game") as the front door.

OWN-WORLD: Catppuccin Mocha/Latte as the base palette, with three surface levels: crust frame (header, activity bar; the status line renders a step darker, gpui-component's status_bar fill), mantle side panes, base work area, surface0 raised cards and inputs. Restrained color: lavender accent only for the one primary action per surface (Build mod, Build & install, Open) and the selection. Category tones (magenta, yellow, green, blue, cyan, red) only as small task icon plates on the start page. Sentence-case pane titles at 14 pt semibold, 14 pt list rows (12 pt meta and hints), system UI face, the platform monospace for ids/paths/code. Lucide icons.

STORY: Open the Studio, and your Launcher games are listed; one click opens one. The start page asks what you want to change (champion look, stats and cards, sounds, map, menus, something new). Champions lists every champion by name; a champion shows skins, abilities, cards and game files, each a click from where it is edited. Every change lands in "your mod" (an unnamed draft until named), never in the game's files; ⌘S keeps it; Redo exists. Build mod leads to one bar: Build & install, Remove from game, Play in Tempest Launcher.

FIRST VIEWPORT: Start page at 1440x900: left column "What do you want to change?" with six task rows (icon plate, title, one-line detail, chevron); right column a "Your mod" card (name, state, Build mod), "How it works" in three numbered steps, recent mods. Header: back/forward, game folder, mod chip, Create, search (⌘K), undo/redo, Changes (n), Build mod (primary).

FORM: Native editor shell (activity bar + panes + palette), the incumbent structure kept and re-taught: Start, Champions, Game files, Game data, Sounds, Maps, then Build mod. Authority for the world: the user's answer of 2026-11-16, "Catppuccin as a base" (keep the palette family, define a stronger editor system on top). No concept roll: the world was pinned by that answer.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
