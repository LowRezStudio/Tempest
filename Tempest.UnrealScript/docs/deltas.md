# UnrealScript: deltas

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Deltas (`delta.rs`, `mcts/delta.rs`, `basestore.rs`)

What a modded file changed, as a `.tdelta` the launcher applies over the game's file ([installer mod
rules](../../Tempest.CLI/docs/mods.md)). Container: `TDLT`, u32 version (1), u32 JSON length, JSON
header, u32 image length, image. Package deltas (`kind: "upk"`): the image is a **mini package**
whose exports are the added/changed objects (payloads remapped into its own names/imports) and whose
imports are every object they refer to, real imports and base-package exports alike, each described
in the header's `refs` (`{"k":"import"}` or `{"k":"export","path","class"}`, resolved by path at
apply time); `ops` lists `add`/`set` (with the mini export index and, for `set`, the canonical hash
of the base object) and `delete` by path. Objects are compared in a **canonical form** independent
of the tables (names as text, references as `class|path` descriptors in order of first appearance,
the written payload): a rebuilt script package differs only where its sources did. A null class
index and an explicit `Core.Class` import mean the same (UClass); the applier writes null like the
cooker. Script objects are remapped through `visit::visit_script` (bytecode disassembled,
`Token::map_refs`, reassembled; struct defaults' value bytes are not remapped). Apply
(`delta::apply`): deletes first, then every op's export is reserved (existing by path, or appended),
then table entries and payloads are remapped into the target; a delta that adds classes is applied
in two phases (`Phase::Script`, class database reloaded from a temp save, `Phase::Objects`) so the
new classes' objects decode (`apply_deltas` in `upk.rs`). **Size** (version 2): `patch` ops
(`property_patch`: same class/super/archetype/flags, the object without its properties canonically
equal, unique property keys, no raw values; the mini export holds the object's header with only the
changed properties and an empty native part; `patch: {remove: [[name, index]…], after: [[name,
index] | null…]}`; apply parses the target object and replaces, inserts after the recorded
neighbour, or removes by `prop_key`, without touching its table entry; a missing object is skipped
with a warning). `from` ops (`pack_from`/`unpack_from`: zstd level 22, window 2^27, long matching,
the base export's raw bytes as raw-content dictionary; chosen when under 85 % of `pack` of the
payload alone; `from: {hash: md5 of the base bytes, size}`, the compressed payload in the file's
trailing section `u32 count, (u32 export, u32 len, bytes)…`; `apply_with(…, original)` takes the
dictionary from the pristine package by path (`upk`'s `apply_deltas` passes a clone of the package
before any delta when one has `from` ops), else from the target's current object; a hash mismatch
skips the op with a warning). `pack`/`unpack`: LZMA2 preset 9e, dictionary sized to the data, `.xz`
stream recognised by its magic, used for the mini image and object files when smaller. Measured on
8.1: a CDO property change 20.9 KB → 0.7 KB, a re-imported body mesh 1.17 MB (zip deflate) → 0.33
MB, another mesh in its place 1.1 MB → 0.42 MB, a texture with a small area repainted 22 KB → 3.2
KB, a texture replaced whole 17 KB → 13.5 KB. **Compression modes** (`Compression::Small` = `pack`,
`Compression::Fast` = `pack_fast`: zstd level 12, long matching, window up to 2^27;
`CreateOptions.compression`, `Delta.compression`, `pack_all_with`, `upk delta create --fast`,
`mod.cook {compression: "fast"}`; `from` payloads use zstd 12 instead of 19 in fast mode; `unpack`
recognises the zstd magic `28 B5 2F FD` as well as `.xz`). DEVICE10772: 23.4 MB in 16 s vs 30.5 MB
in 4 s; Cassie Bow cooked 61.1 MB in 22 s vs 79.2 MB in 9 s. `pack` is LZMA2 preset 9 (9e measured
the same size, slower; zstd on all 89 parts of Cassie Bow, 214 MB: LZMA 59.9 MB in 71 s CPU, zstd 19
70.1 MB in 65 s, zstd 22 70.0 MB in 74 s, zstd 16 74.8 MB in 39 s, zstd 12 77.9 MB in 9 s, so LZMA
stays), dictionary = the input rounded up to 1 MiB (at most 256 MiB), one stream on one core;
parallelism comes from **streams** instead: after the `from` decisions, payloads of 4 KiB or more
are grouped by `stream_key` (the class, and for textures the kind from the name: diffuse, normal,
mask, specular, emissive, other) and each group of at least 1 MiB (`STREAM_MIN`) becomes a stream
outside the image (`Delta::streams`, op `stream: [s, offset, len]`, trailing file section `u32
count, (u32 len, xz)…`, decoded in parallel). Measured on `DEVICE10772_ASSETS_SF` (101 MB of
payloads): one stream 23.24 MB in 38 s; blocks of 8/16/32 MiB lost 9/3/22 %; grouped by class 23.20
MB, textures by kind 18.57 MB vs 18.59 MB together (halving a kind lost 20 %, so kinds are never
split); with streams 23.35 MB in 16 s. `Delta::parts`/`assemble` let callers compress many deltas'
parts in one `pack_all` pool (biggest first, a 6 GiB budget at about 12 × dictionary per encoder).
`from` is decided by a zstd level-9 pass with and without the dictionary (`worth_from`, under 80 %)
before the level-19 pass, all candidates in parallel (`parallel_map`). `CreateOptions { fast }`
(previews: no `from`) and `share_only` (share only these hashes; other large objects stay in the
delta). The cooker (`mod_cook`): a discovery pass (`Sharing::All`, `fast`) finds the objects two or
more items hold, then each item is made once, sharing only those (`Sharing::Only`), package diffs in
parallel (`mod_items`/`package_item`), every part of every delta and the new object files encoded in
one `pack_all` (`encode_pending`, `Payload::Delta` until then); results are cached for the session
by `item_stamp` (source, target, launcher backup sizes and times, base record) and sharing set.
`mod_preview` uses the same parallel path and a cache. Cassie Bow (5 packages, 230 MB edited, 1300
objects): 61 MB (74.6 MB without sharing, 80.8 MB sharing every large object), cooked in about 25 s
(it was 80 s), under a second when rebuilt unchanged; the launcher applies it in 4.6 s, identical to
the edited packages. **Shared object files** (version 2, `CreateOptions { share_min }`,
`create_with`): a payload of at least `share_min` bytes (`SHARE_MIN`, 16 KiB) is not stored in the
mini package; it becomes a one-export package of its own (`shared_summary`: the edited summary
without folder name, GUID, `package_source` checksum or chunks; names and imports in order of first
appearance through the same `map_ref`/`Remap`, with only the object itself as an export, so
references to the delta's other objects are path descriptors), named by the SHA-256 of its bytes.
The op keeps its mini export (table entry, empty payload) and gets `shared: {hash, refs}`; apply
parses the payload from the object file and resolves it with a `Resolver` over the file and those
refs (`target_of` = the op's export). `to_bytes` embeds the object files (one self-contained file),
`write_shared(path, store)` writes them as `<store>/<hash>.tobj` (once) and the delta without them,
`Delta::read` loads embedded ones or finds `shared/<hash>.tobj` in the delta's folder or above (a
mod's `deltas/…` tree) and checks the hash. Version 1 is written whenever no op is shared. Checked
by `delta verify --share --share-min 1` (every object shared) on content, a mixed edit with added
objects referring to each other and a class default object of `TgGame.upk`; Maeve's body mesh
replaced in `MAEVE_ASSETS_SF` and `DEVICE16218_ASSETS_SF` gives one 1.8 MB object file for both
deltas. Marshal deltas (`kind: "mcts"`, no image): per top-level row set the key column (first
`*_ID` field unique across the rows) and `add`/`set`/`delete` by key with the row's JSON
(`serde_json` is built with `preserve_order` so rows come back byte-identical), plus changed header
fields. `basestore.rs` is the Studio's record of a file before its first in-place save: the digest
(`delta::digest` / `mcts::delta::digest`, canonical hash per object or row) under
`<cache>/tempest-studio/base/`, so `mod.dir` / `mcts.scaffold` make deltas without the original file
(`delta.status|record|reset|create|info` in `serve.rs`). CLI: `upk delta create
<base.upk|digest.json> <edited> -o out [--target] [--shared <dir> | --share] [--share-min <bytes>]`,
`apply <pkg> <d>... -o out [--strict]`, `info`, `digest`, `batch <plan.json>` (the launcher's
resolver: jobs `{base, deltas, out, dir, game}` in one process, events as JSON lines on stdout:
`progress` with the fraction of the plan, `warning`/`drift`, `done`/`error` per job), `verify <base>
<edited>` (create + apply + canonical compare; the test, run on content, recompiled script packages,
`assembly.dat` and `Lang_*.dat`). The class database must be the game's: `create` warns when an
object's class is unknown (its references would be copied as indices), and the CLI picks the first
given package's folder that holds `Core.upk`.

Wwise package deltas (`kind: "pck"`, `wwise/delta.rs`): no image; the header's `ops` name table
entries or bank parts (paths above, what the launcher's conflict check reads as `ops[].path`), each
`add`/`set` payload is a slice of one stream (`stream: [0, offset, len]`, `size`), `set`/`delete`
carry the MD5 of what they replace (`base`, drift when it differs: the delta wins). A bank is taken
apart only when it parses and its `HIRC` count matches the objects read; otherwise it is one
`bank:<id>` op. Embedded media changes rebuild `DIDX`/`DATA` in the 16-byte layout
(`Bank::set_media`), objects are written back with `Bank::set_objects`. `upk delta
create|apply|digest|verify <file.pck>` and `upk delta batch` (the launcher) take `.pck` files; on
the game's `WWB_MUS_Lobby.pck` a replaced menu song (4 objects + 1 stream) is a 3 MB delta that
rebuilds the edited file byte for byte.
