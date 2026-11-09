# Studio mod projects, workspaces, saving and undo

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

- **Workspace files** (`.twrk`, `src/lib/studio/session.svelte.ts`, `twrk.rs` +
  `twrk.save|read|apply`): the session's unsaved packages and `.dat` files plus where the user was
  (open package, selection, assembly table and row, tabs, history, pane layout, id range, cooker
  details), independent of saving in place or Save as. **Linked** files hold `.tdelta`s against the
  game folder's files (kilobytes; script packages and files whose delta has warnings are stored
  whole); **portable** files hold every changed file whole, the packages opened from outside the
  game folder and the recorded bases (opens on another machine or game copy; a missing folder is
  asked for). Opening one reopens the game folder, applies the changes in memory (unsaved, undoable
  as one "load workspace" step) and restores the state; Ctrl+Alt+S saves to the current file
  (`SaveWorkspaceDialog` picks the mode for a new one), Ctrl+Alt+O opens; `.twrk` is a file
  association. A linked recovery copy is written every 2 minutes while anything is unsaved
  (`<userData>/recovery/<hash of the folder>.twrk`, `workspace:recovery`), offered in a banner when
  the folder opens again, deleted once everything is saved.

- **Mod projects** (`NewProjectDialog.svelte`, `session.createProject`; welcome screen and the
  header menu): a wizard (mod name with the id made from it, author, version; the game folder; where
  the project lives, default `Documents/Tempest Mods/<id>`) creates `<id>.twrk` (a linked workspace
  file), `mod-files/` (shipped as laid out: `scripts/`, `dlls/64/`, `files/…`; a README explains it)
  and `builds/`. In a project (`session.project`, kept in the workspace state) every edit is kept in
  the project file, which saves itself 1.5 s after a change (`studio.edits`, `assembly.edits` and
  the cooker's state are watched; `projectState` saved/pending/saving/error is shown next to the
  project name; a running save is waited for); Ctrl+S saves the project, "Write the edits into the
  game folder" is a separate, confirmed command, and the header's main button is Build mod. Recent
  projects and game folders (welcome screen, header menu) can be removed one by one or cleared
  (`session.forgetRecent`, `studio.forgetRecent`); a project file that no longer exists is dropped
  when opening it fails. **Closing** the project (`session.closeProject`, also before opening or
  creating another) asks first: Save and close / Discard and close when a save is pending or failed,
  else Close; then the project's edits are unloaded from memory (`package.discard`, `mcts.discard`),
  so nothing of the project can be saved into the game folder by mistake, and come back when it is
  opened again. The project lists only the game files edited while it is open or added to it
  (`cooker.targets`); builds go to `builds/<id>-<version>.tempest` (asks before replacing a version
  already built).

- **Mod cooker** (Mod view, Ctrl+3; `ModView.svelte`, `src/lib/studio/cooker.svelte.ts`,
  `mod.changes|preview|cook|match|addCopy|forgetCopy`, `electron/src/cli.ts`): a mod is the list of
  game files it changes. `mod.changes` returns every source of edited content, each naming its game
  file (`target`): unsaved edits (taken from memory), game files saved in place since the Studio
  recorded their original, and exported copies (Save as of a package or `.dat` is a copy: the edits
  stay unsaved in the Studio and the copy is remembered as a modified version of its game file in
  `<cache>/tempest-studio/copies.json`, `copies.rs`, the game file as the workspace names it and the
  copy canonicalized without Windows' `\\?\` prefix; the cooker compares paths with
  `pathKey`/`samePath` from `format.ts`, separators and case aside; files added with "Add a modified
  file…" or as other files with a `.upk`/`.dat`/`.pck` name are matched to the game file of the same
  name, `mod.match`, and registered the same way). Per game file the user picks the source (default:
  unsaved, else the newest copy, else the in-place save) and delta or whole file. A delta compares
  with the original: the Studio's record (`basestore`), else the launcher's pristine backup
  (`<game>/.tempest/v2/backup/<path>` or `v1/backup/<name>`, `basestore::launcher_backup`), else the
  game folder's own file, which is taken as the original by default (`delta.record {path, from}` can
  still set one from another file). `mod.preview` shows what each delta holds (changed/added/removed
  objects or rows); files saved in place earlier that no longer differ are left out of the list. The
  view has What goes in (one line per game file with its change count, click for the list, a source
  picker only when there are several, and an action menu: show the changes, leave out, ship the
  whole file (not for script packages), discard my edits (confirmed), remove from the project or,
  outside a project, from the list (`cooker.dismiss`: a file saved in place stays hidden until it is
  written again, `dismissed` holds its `modified` time from `mod.changes`), forget a copy; the
  project's `mod-files/` with its file count; added files with their path inside the mod), Details
  and Build; added files with their path inside the mod (Lua → `scripts/`, DLLs → `dlls/64/`,
  `.upk`/`.dat`/`.pck` become modified game files). The checks match the launcher's cooker
  (`src/lib/studio/modmeta.ts`, a port of the launcher's `packaging.ts`/`deps.ts`): Details: name,
  id (`MOD_ID_PATTERN`, made from the name unless typed), version (`isSemver`, no build metadata;
  `toSemver` offers a one-click fix), authors with links; Listing: summary (≤ 140), tags, source
  link (`--repository`), dependencies (one per line, `dependencyProblem` checks the repository's
  requirement grammar; `tempest-scripting>=1.0.0` is added automatically when the mod ships
  scripts); Icon and readme: icon (`iconProblem`: a real PNG, ≤ 1 MB, previewed) and a Markdown
  readme. Invalid values, and code without a signing key, are listed as `problems` that block Build.
  Kept per game folder and in workspace files; New mod clears them. Cook writes the mod folder
  (manifest in the launcher's format; large objects that several files change the same way as shared
  object files, written once, the build log says how much that saved), then `tempest-cli mod pack`
  and `mod sign` (the view creates a key with `mod gen-key` when missing), then Install (`mod
  install <game> <pkg> --replace --allow-untrusted`; warns when the folder holds in-place edits of
  files the mod changes), Save a copy (`fs:copy`) or Publish… (`PublishDialog.svelte`: `mod publish
  --json` with `--patch` per listed patch or `--from/--to`,
  `--summary/--changelog/--tags/--repository`; the outcome parsed like the launcher's `repoPublish`:
  done with a link, queued, failed with errors, warnings; an auth error asks for an API key in
  place, `mod login`, and publishes again). Signing and publishing (`ModAccount.svelte`,
  `cli:account` reads the CLI's `signing/tempest.key` through `mod fingerprint` and `repo.json`):
  the signing key (create with `mod gen-key --id`, verify ownership with `mod sign-text`) and the
  API key (masked; log in, replace, forget with `mod login` without a key), shared with the
  launcher. The CLI is `$TEMPEST_CLI`, the bundled `binaries/tempest-cli` (Studio build and
  `studio-release.yml` publish Tempest.CLI next to `upk`), or the repository's Debug/Release build
  in dev; only `mod` subcommands run. The launcher keeps its own cooker.

- **Saving and discarding**: Save writes into the game folder (the Studio records the original
  first) after a warning that offers a workspace file instead (`confirmInPlace` in `actions.ts`, for
  Save and Save all; `ui.confirm` takes `remember`, a "Don't ask me again" kept in localStorage);
  Export a copy (Save as) writes elsewhere and leaves the edits unsaved; Save workspace file keeps
  everything for later; the Save menu describes each (`Command.detail`, shown in menus and the
  palette). Discard (header button) and Discard all ask through the shared confirmation dialog
  (`ui.confirm` → `ConfirmDialog`), as do deleting exports and rows. Unsaved files show a dot in the
  package list and on tabs; the header's unsaved count opens the list of unsaved files; the package
  list marks game files with exported copies ("copy"), `studio.marks` from `mod.changes`; files
  saved in place get no mark.

- **Undo** (Ctrl+Z outside inputs, `package.undo` / `package.undoList`): every mutating call starts
  an `Editor::begin_undo(label)` entry that snapshots the exports it touches (table entry, payload,
  parsed object, dirty flag), the table lengths and the summary; a script recompile snapshots the
  whole package. Undo is LIFO, 50 entries, and survives saving in place: the package stays open with
  its journal (`Editor::mark_saved` flags the entries as on disk, so undoing one of them dirties the
  package again and the next Save writes the reverted state); it is lost when the package is closed.
  **Discard** (`package.discard`) drops the in-memory package and reloads it from disk.
