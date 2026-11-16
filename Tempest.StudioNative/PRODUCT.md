# Product

<!-- impeccable:product-schema 1 -->

## Platform

Desktop, native (Rust + GPUI, gpui-component). macOS first; Windows and Linux builds exist.
Not web, not mobile. Pointer + keyboard, windows from ~1000 pt wide upward.

## Product

Tempest Studio (native) is the editor for making Paladins mods. It opens the game's
folder, lets you look at and change anything in it (textures, meshes, materials,
animations, particles, sounds and Wwise banks, UI movies, maps, the content tables in
`assembly.dat`), and turns those changes into a signed mod the Tempest Launcher installs.
The game's own files stay untouched: edits live in the Studio and in mod projects.

## Users

- **Primary: people who have never made a Paladins mod.** They play the game, want to
  change something (a skin, a number, a sound, a map), and have no idea what a package,
  an export, a marshal table or a cooker is. Success: they make their first change and
  build a working mod without reading docs or asking in Discord.
- **Secondary: experienced modders and the maintainer.** Daily, long sessions; they rely on
  keyboard shortcuts, the command palette, multiple tabs and windows, MCP automation.
  The redesign must not slow them down or hide power.

## Jobs

Newcomers come for any of these, roughly equally ("pretty much everything to mod the
game, it varies"):

- Reskin a champion, weapon or model (textures, materials, meshes).
- Tweak stats, cards, items, talents (content tables).
- Replace sounds, voice lines, music, and the menu/HUD movies.
- Edit maps (props, lighting, collision, nav mesh, Kismet, Matinee).
- Then: package it as a mod (details, icon, signing), install it, publish it.

## Constraints and truths to preserve

- Every user action is a `Cmd` in `commands.rs`, shared by keys, palette, menus and the
  shortcut list. Object picks go through the palette.
- Edits are kept in memory with undo, autosave of mod projects, recovery; writing into the
  game folder is the exception (quick tests), never the default path.
- Behavior and file formats match the Electron Studio and the backend; a redesign changes
  presentation, wording and flow, not editor semantics.
- Never name the unreleased 8.2 server build in anything user-facing.

## Brand commitments

- Catppuccin (Mocha dark / Latte light) is the palette base, shared with the Tempest
  Launcher and the Electron Studio. The editor may define its own system on top of it
  (surface levels, accent use, type scale, density), confirmed 2026-11-16.
- Lucide icons (registered set).

## Voice

Plain words a player understands, then the technical name for those who need it
("Content tables (assembly.dat)"). Short, direct, no jargon without an explanation nearby.

## Open decisions

- Whether newcomer guidance is a dedicated first-run flow, task-based entry points, or
  contextual help inside each view: to be decided in the redesign plan.
