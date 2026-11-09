# Mod deltas, signing and trust

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Mod deltas

A `.tempest` package changes a game file either whole (`files/<game path>`, replaces the file; two
mods on one file conflict) or by **delta** (`deltas/<game path>.tdelta`: only the objects or rows
the mod added, changed or removed, keyed by object path / table id column, never by index). Deltas
of several mods on one file compose; conflicts are per object (`ModV2Installer` compares the keys of
the delta files, `DeltaResolver.Keys`) and the usual replace/stack prompts apply, stack meaning
"last installed wins" on the shared objects. The game file is derived:
`Tempest.CLI/Mods/DeltaResolver.cs` rebuilds it as *base + every enabled mod's delta in install
order* with the `upk` sidecar (one `upk delta batch` run for every file to rebuild, so the class
database loads once; found next to `tempest-cli`, `TEMPEST_UPK`, or the repo's
`Tempest.UnrealScript/target/release/upk` in dev) after every install, remove, enable and disable,
and at launch, printing `[progress] 42% <what>` lines on stderr while it works (the launcher's mod
rows show them as "Patching game files 42%", streamed through `shell:execute`'s `streamId`); the
base is the pristine file kept as the `.tempest/v2/backup/` copy (made when a delta first touches
the file), or the copy of the one mod that replaces the file whole. `.tempest/v2/deltas.json`
records the stack and output hash per file so an unchanged stack costs one hash; when no delta is
left the backup is restored. `ModRecord.DeltaFiles` lists the targets (counted as owners by
`ModIntegrity`, so the game copy is not compared to a snapshot, and hashed by `PackageHashes` like
owned files). A `set` whose base object no longer matches (another mod changed it first) is applied
anyway and reported as drift (`[deltas] note:` on stderr). Deltas are as small as the change: an
object whose change is only in its tagged properties ships just those properties (a `patch` op,
merged into the object as it is at apply time, so two mods changing different properties of one
object compose); a large payload is stored compressed against the base object's bytes (zstd with the
base as dictionary, a `from` op, used when clearly smaller; it needs that original object, which the
resolver's pristine backup provides, and is skipped with a warning when the game file differs); the
mini package and object files are LZMA-compressed. Large objects (payloads of 16 KiB or more) can be
**shared object files** instead (delta version 2): `shared/<sha256>.tobj` at the mod root, a
one-export package whose bytes depend on the object only, so an object a mod changes the same way in
several packages (Maeve's mesh in her `_SF` package and a device's) ships once and every delta
refers to it by hash; the Studio's cooker finds (uncompressed) which large objects two or more of
the mod's files contain and shares exactly those (sharing every large object made mods bigger:
objects compress better next to the objects like them); package diffs run on every core (`mod_items`
→ `package_item`), every compression job of every delta and object file goes through one pool
(`pack_all`: biggest first, within a 6 GiB encoder-memory budget), finished items are cached per
source/target/backup stamp and sharing set for the session (`cook_cache`, `holds_cache`,
`packed_cache`: a rebuild with nothing changed takes under a second); `mod.preview` never
compresses, runs its diffs in parallel and is cached the same way. Cassie Bow (5 packages, 230 MB):
61 MB in about 25 s. The Build card's **Compression** choice (`meta.compression`, `mod.cook
{compression}`): "Smallest file" (LZMA, default) or "Faster build" (zstd 12: Cassie Bow 79 MB in
about 9 s); readers detect either by its magic, `upk delta create --shared <dir>` on the CLI. The
launcher stores `shared/` in the installed mod's folder like any other entry and `upk` finds it
above the delta (`Delta::read`); `DeltaResolver.ReadHeader` accepts versions 1 and 2; deltas are
version 2 whenever they are compressed (almost always), so mods made now need an up-to-date
launcher. Format, `upk delta create|apply|info|digest|verify` and the Studio's base records:
[backend delta reference](../../Tempest.UnrealScript/docs/deltas.md). Signing is unchanged: deltas
are ordinary entries in `CHECKSUMS`, and the code policy below does not treat script packages as
code.

## Mod signing

`.tempest` packages carry `CHECKSUMS` (sha256 per file), `CHECKSUMS.asc` (OpenPGP signature over it)
and `SIGNER.pub` (the signer's public key, listed in CHECKSUMS). `tempest mod install` policy
(`Tempest.CLI/Mods/ModV2Installer.cs`): a package with code (any PE image, or `.lua` other than
`.d.lua` stubs) installs when signed by a trusted key, needs `--allow-untrusted` when validly signed
by an unknown key (the launcher's `UnverifiedModDialog`: for packages from the mod repository,
`fromRepository` through `installModFile` / the install queue, it says the code was reviewed by
moderators but the signer is not vetted and a review can miss things; for local files it keeps the
stronger "Untrusted Code Warning"), and is refused when unsigned, invalidly signed or signed by a
revoked key. Packages without code install regardless. Packages may not write into
`<game>/.tempest/` or outside the game/mod folder. Entry paths are matched to the game folder's
casing before anything is written (`Mods/GamePath.cs`: `files/…` and `deltas/<game path>.tdelta`
entries, and the v1 installer's file name), so a mod naming
`chaosgame/cookedpcconsole/maeve_assets_sf.upk` replaces the game's `MAEVE_ASSETS_SF.upk` on
case-sensitive file systems (Linux) instead of creating a second file; parts that do not exist keep
the mod's spelling. Checksums and the code check still use the names as stored.

Trust (`Tempest.CLI/Mods/TrustStore.cs`): `keys/trusted.keys` (armored public keys) and
`keys/untrusted.keys` (fingerprints / long key IDs) are compiled into the CLI and published to
`https://cdn.lowrezstudio.com/keys/` (R2 bucket `lowrezstudio-cdn`, rclone remote `lowrezstudio`)
with `.asc` signatures by a list signer (Tom, Kyiro). Online: built-in + published trusted keys +
`*.pub` in `<game>/.tempest/keys/` and `<app data>/com.lowrezstudio.tempest/keys/`. Offline or
unverifiable lists: built-in keys only, plus the last verified untrusted list cached in `<app
data>/com.lowrezstudio.tempest/keys-cache/`. Revocations always win; at launch, enabled mods signed
by a revoked key are disabled, and DLLs of mods installed unsigned are not injected.
`TEMPEST_KEYS_URL` points the CLI at another URL or a local folder (testing). To trust or revoke a
key: edit `keys/*.keys`, then `just keys-publish`; no release needed.

The client refuses script packages (and the startup maps) whose SHA-1 differs from the table in its
executable (RCDATA 1010, "SHA Verification failed ... Bad hash"). At launch,
`Tempest.CLI/Launcher/PackageHashes.cs` writes the hash of every listed file an enabled mod replaced
into the started process, so mods can ship recompiled script packages; Windows only for now (Wine
launches print a notice).

The private signing key lives at `$TEMPEST_SIGNING_KEY`, else `<app
data>/com.lowrezstudio.tempest/signing/tempest.key`; never commit it. CLI ↔ repository: `tempest mod
search`, `mod login <api key>`, `mod publish <pkg> --from <patch>`, `mod install <game>
<id>[@version]` talk to mods.lowrezstudio.com (`Tempest.CLI/Mods/ModRepoClient.cs`, config in `<app
data>/com.lowrezstudio.tempest/repo.json`, `TEMPEST_REPO_URL`/`TEMPEST_REPO_KEY` override). Recipes:
`just mod-keygen`, `just mod-pack <dir> <out>`, `just mod-sign <pkg>...`, `just mod-sign-text
<text>` (detached signature over a text, used to verify a signing key on mods.lowrezstudio.com),
`just mod-verify <pkg>`, `just keys-sign`, `just keys-publish`, `just keys-status`; `just
scripting-build` signs its packages when the key exists.
