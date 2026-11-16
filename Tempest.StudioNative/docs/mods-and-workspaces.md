# Native Studio: mods and workspaces

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Mod cooker

Mod view (`cooker.rs`, `account.rs`, `modmeta.rs`): a bar with the mod's name, what blocks a build
(else what goes in or the last build), Name this mod… (an unnamed mod), Remove from game (`mod list
--json` finds the record by id, then `mod remove`), Play in Tempest Launcher (`tempest://`: the
Launcher owns launching), ⋯ (Build only…, Install here anyway, Install elsewhere…, Save a copy…,
Publish…) and **Build & install** (a fast build, into a temporary folder for an unnamed mod, then
`mod install` into this game unless files saved in place would hide whether the mod works); tabs
Changes (what goes in, each file named in words with what changed listed under it: `mod.preview`
runs for the first 8 files after every refresh, errors shown in the file's card; other files, the
last build's log, problems), Details (name, author,
version, icon, readme; Advanced: id, dependencies, Build only's compression) and Publish (listing,
Publish…, signing and account). Underneath: `mod.changes` per game file (re-read whenever
`backend::EDITS` moves), source and delta/whole choice, `mod.preview`, a `…` menu per file (show the
changes, leave out, whole file, Discard my edits, Forget the copy, remove from the list or project:
`dismissed` by modified time), Other files (Add files…: Lua → `scripts/`, DLLs → `dlls/64/`,
`.upk`/`.dat`/`.pck` matched with `mod.match` + `mod.addCopy`, else shipped as a new file), Details
(name, id, version with a `to_semver` fix button, author and link), Listing (summary ≤ 140, tags,
source link, dependencies checked by `dependency_problem`; `tempest-scripting>=1.0.0` added when Lua
ships), icon (PNG ≤ 1 MB) and readme; the checks of the Electron `modmeta.ts`, unit-tested, block
Build (code without a signing key too). Build (`mod.cook` with icon/authors/deps/extras +
`tempest-cli mod pack/sign`; in a project into `builds/`, asking before replacing a version),
Install, Save a copy, **Publish…** (`mod publish --json` with patches/range/changelog/listing,
outcome parsed, an auth error asks for an API key in place and publishes again), **Signing and
publishing** (key fingerprint from `mod fingerprint`, Create my signing key = `mod gen-key --id`,
Prove it is my key = `mod sign-text`, API key log in/replace/forget = `mod login`, read from
`repo.json`). The details live in the project file (`Project.cooker`) or per game folder. Driver:
`cooker build` and `cooker publish-dialog` (never publish from a test).

## Workspace files

Workspace files (`workspace.rs`: `twrk.save` linked/portable, `twrk.read`/`twrk.apply`, a `.twrk`
argument opens one).

## Mod projects

**Mod projects** (`project.rs`): New/Open/Close project, a folder with `<id>.twrk` (linked;
`state.project` holds name, id, author, version and the edited file names), `mod-files/`, `builds/`,
`README.txt`; the file saves itself 1.5 s after the last edit (`backend::EDITS` counts successful
mutating calls), ⌘S saves the project and Write into the game folder is its own command, the header
shows the project chip (save state, menu) and Build mod; closing saves or discards, then unloads the
edits (`package.discard`, `mcts.discard`); the cooker lists only the project's files (plus unsaved
ones), takes its details, ships `mod-files/` as an extra and builds into `builds/`.

Mod view build options: **Compression** (smallest file = LZMA, faster build = zstd; `mod.cook
{compression}`, kept with the mod's details), **Add folder…** (shipped under its name), **Install
into this game** (asks first when files of the mod were saved in place here) and **Install
elsewhere…** (another copy of the game). Workspace files keep the id range for new assembly content
(`idRange`).
