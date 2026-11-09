# UnrealScript: hot reload

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

### Hot reload (`live.rs`)

`upk live baseline <cooked> <state> [result.json]` and `upk live changes <request.json>
[result.json]` serve the `upk-hotreload` Lua mod, which swaps changed movies into the running game.
The state folder holds `index.json` (stamp = size + mtime and movie count of every `.upk`) and, per
package with movies (8.1: GameUI_SF, LobbyUI_SF, Startup, GFxFonts, IME, TgClient), `<file>.json`:
every movie's `RawData` FNV-1a 64 (the mod hashes the bytes in memory the same way), size and
external image stems, and every other object's canonical hash (`delta::canonical`) plus a
fingerprint (FNV-1a 64 of the export entry and payload). `RawData` is read from the tagged
properties without the class database (`read_object_header`; equal to `media::swf_export` on all 569
LobbyUI_SF movies). A new version reuses the recorded canonical hash of every object whose
fingerprint is unchanged when its name, import and export tables only grew (`Tables`, running FNV
over the entries; the Studio editor and the delta resolver append), so the class database is loaded
only when a non-movie object changed (a movie-only change of LobbyUI_SF, 113 MB: 0.85 s; with a
texture: about 3 s; `UPK_LIVE_DEBUG=1` prints what had to be parsed). Placeholders (deleted exports,
all named `none`) are kept with an empty hash under `none#<index>`. `changes` takes `{state, out,
scripts, packages}`, writes changed movies to `<out>/<pkg>/<path>.gfx` and returns per package
`changed` (path, `old`/`new` hash, `copies`: the other packages' recorded hashes of that path),
`added`, `removed`, `textures` (changed Texture2D named by an external image, with the movies naming
them), `other` (counts by class), plus `movie_packages` with up to 12 paths unique to each package
(one of them in memory means the package is loaded). A package without a usable record lists every
movie with `old: null`. Errors go into the result file as `{"error"}` (the mod has no console).
Format version 3; a state that does not read back is recorded again.
