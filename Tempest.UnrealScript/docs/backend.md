# UnrealScript: backend

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Studio backend (`serve.rs`, `json.rs`)

Studio workspace files and the mod cooker in `serve.rs`: `twrk.save {out, mode, state, include}`
writes a `.twrk` zip (`twrk.rs`: `workspace.json` + `deltas/` + `files/` + `bases/`; linked mode
makes each package delta from the bytes Save would write, `Package::parse(&ed.bytes())`, since the
in-memory package can differ from its saved form), `twrk.read` returns the manifest, `twrk.apply`
applies it to the open workspace (deltas through `delta::apply`, whole packages through
`Editor::replace_package`, marshals replaced or delta-applied, portable-only external packages
unpacked under `<cache>/tempest-studio/twrk/`, carried bases put back with `basestore::restore`).
`mod.changes` lists every source of edited content per game file: dirty editors, dirty marshals,
files with a recorded base (`basestore::all`) and exported copies (`copies.rs`; `package.save` /
`mcts.save` with `out` write a copy with `Editor::save_copy`, keep the edits unsaved and register
it), each with its base (`recorded`, launcher `backup`, else `game`: the game folder's own file is
taken as the original); `mod.preview` and `mod.cook` take `{source, target, kind, mode}` items
(`mod_item`: content from memory when the source is open and dirty, else the file; a delta against
the recorded base, the launcher backup or the game folder's file; a target missing from the game
folder must be shipped whole); `mod.match` finds the game file a file from anywhere is a copy of;
`mod.addCopy` / `mod.forgetCopy`; `delta.record {path, from}` records a base from another file.
`mod.cook` writes a launcher mod folder (manifest like the launcher cooker's `buildManifest`,
README.md, icon.png, `deltas/` (made with sharing on; a delta none of whose object files another
delta uses is made again without sharing, so it stays version 1; object files written once into
`shared/`; the result's `shared {files, uses, bytes, saved}`), `files/`, extra files; an extra
folder with an empty `dest` is copied into the mod root as laid out, skipping dotfiles,
`manifest.toml` and `.tempest`, which is how a Studio project's `mod-files/` ships). `object.list`
(`Editor::edit_list`, `ListOp`: add/duplicate/remove array elements, remove a property) and
`object.duplicate` (`Editor::duplicate`). `mcts.addRow` (`Workspace::append_row`: fresh id from the
table's named id column, `renumber` of nested sets, message copies, or a pasted JSON row) and
`mcts.usages`. `delta::apply` judges drift before running the deletes (a delete changes the
canonical hash of objects that referred to the deleted one).

`upk serve` is a JSON-lines RPC server over stdin/stdout for `Tempest.Studio`: one request per line
(`{"id", "method", "params"}`), one response per line (`{"id", "ok", "result" | "error", "ms"}`),
plus id-less event lines (`{"event": "search.progress", ...}`). It keeps the `ClassDb` (leaked to
`'static` on `workspace.open`, so editors can borrow it), the `TextureSource`, one `Editor` per
opened package (keyed by canonical path) and a per-package search index in memory (built on a
background thread from `workspace.open`, ≈2 s for a full cooked folder, joined by `index_all` before
the first search, batch plan or textured mesh/material preview). Methods:
`workspace.{open,packages,search,summary}`, `package.{open,tables,close,dirty,save,rewrite,verify}`,
`object.{get,text,set,rename,delete,copy,deps,extract}`,
`texture.{info,preview,export,exportAll,import}`, `mesh.{info,preview,export}`,
`media.{preview,export,import}`, `mod.dir`. `script.{source,compile,export}` drive
`tempest_unrealscript::toolchain`: `source` decompiles the package once (cached) and returns the
class owning the export; `compile` writes the cached sources with one class replaced to a temp dir,
compiles them conformed to the file on disk (`CompileOptions::for_replacement`, dependencies from
`build_order` of the workspace) and swaps the result into the open `Editor` (`replace_package`,
dirty until saved); errors come back as `compile failed: {"class","line","detail","message"}`; with
`create: true` the class may be new (a valid identifier the package does not have): it is compiled
in with the others and `added` gives its export. `json.rs` renders tables, parsed objects and
property values (`{"t": kind, "v": ...}`; references as `{ref, path, class}`); previews travel as
base64. Saving in place drops and reopens the editor, because the original is memory-mapped. The
TypeScript mirror of the shapes is `Tempest.Studio/electron/src/ipc/contract.ts` (`UpkMethods`):
keep both in sync.

### Serve performance notes
Background work from `workspace.open` on: the search index plus its object-path owners map
(`owners`, `index_lookup` without scanning every export), `assembly.dat` + languages (`mcts_job`),
and a decompile of each script package opened (`script_jobs`, used when the file is unchanged).
`Package::read_summary` reads 64 KiB (4 MiB only on failure); chunks are (de)compressed in parallel
(`par_map`); cross-package lookups for previews share loaded packages between requests
(`material_ops::load_shared`, 768 MiB LRU, invalidated by size/mtime and `forget_shared` on save).
The class database is reused when the same folder is reopened with unchanged script packages.
Compiler/decompiler lookups are cached (`Context` import resolution reset on every package load,
per-struct child names and operator lists, model name index): keep caches keyed on state that only
grows, and recheck byte identity with `scripts/verify.sh` on 8.1 and 0.57. A panic in a request is
returned as that request's error instead of ending the server.

`ClassDb::load_files` parses the script packages in parallel (`Context::load_files_parallel`,
`par::map`; 0.75 s for 8.1's 16 packages) and `list_packages` reads the 993 summaries in parallel.
The search index is persisted compressed under the OS cache folder
(`<cache>/tempest-studio/index/<hash>.idx`, entries reused while a package's size and mtime match).
`upk serve` answers framed after `{"method":"protocol","params":{"frames":true}}`: header line `{id,
ok, ms, len, bin:[…]}` + result JSON + raw payloads (`BinSink`, `{"$bin": n}` markers); without the
handshake payloads are base64 as before (scripted sessions). `package.open`/`package.tables` return
the export table as columns (`json::tables_compact`).
