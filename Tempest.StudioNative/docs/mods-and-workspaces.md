# Native Studio: mods and workspaces

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Mod cooker

Mod view (`cooker.rs`, `account.rs`, `modmeta.rs`), laid out like the website's mod page: a header
with the mod's icon (initials on a tone when it has none), name and facts, Name this mod… (an
unnamed mod), Remove from game (shown once `mod list --json` finds the mod installed here; also in
⋯; `mod remove` by the installed name), Play in Tempest Launcher (`tempest://`: the Launcher owns
launching), ⋯ (Build only…, Install here anyway, Install elsewhere…, Save a copy…, Remove from game,
Publish…) and **Build & install** (a fast build, into a temporary folder for an unnamed mod, then
`mod install` into this game unless files saved in place would hide whether the mod works); tabs
Files (game files you changed, each named in words with its counts on its line: `mod.preview`
runs for the first 8 files after every refresh; What changed unfolds the list of changes under a
file and keeps it unfolded across refreshes; errors shown in the file's row; with two files or
more, a bar above them ticks or unticks every file and removes the unticked ones from the list or
project in one go (`remove_left_out`, after a confirm; a file with unsaved edits always stays
listed, so it is neither counted nor offered in its ⋯ menu); other files),
Details (name, version, author, icon, readme; Advanced: id, dependencies, Build only's compression)
and Publish (listing, game patches, changelog, a pointer to the website for images and visibility,
where the game patches are picked like on the website's upload form: Every patch (from the first
known patch, no end), A range (two clicks on the timeline; Also future patches leaves the end open)
or Specific (clicks pick and unpick), on a timeline of every patch in the Launcher's
`src/lib/data/versions.json` (read at build time, the "Unreleased" entry left out), saved as
`patches`/`patchFrom`/`patchTo` and sent as `--patch`/`--from`/`--to`;
signing and account); a right column on every tab with the mod card as the mod list shows it,
Publish… (Publish tab), the problems with a way to each fix, and the last build with its log.
Underneath: `mod.changes` per game file (re-read whenever `backend::EDITS` moves), source and delta/whole choice, `mod.preview`, a `…` menu per file (show the
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
