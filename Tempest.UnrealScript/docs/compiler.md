# UnrealScript: compiler

[Agent entry point](../AGENTS.md). Read only the sections relevant to the task.

Paths in this reference retain their original project/repository context; verify details against
current sources.

## Commands

```bash
cargo build --release                    # target/release/uscript  (or: just unrealscript-build)
cargo test --release                     # a handful of unit tests
cargo clippy --release --all-targets     # warnings are tolerated, errors are not
just unrealscript-check                  # test + clippy (part of `just check`)
just uscript <args>                      # run the CLI through the task runner
```

CLI (run `uscript` for the full list):

```
uscript info|names|imports|exports <pkg>              header tables
uscript disasm <pkg> <idx|path>                       token tree of one struct
uscript dump <pkg> [out.txt] [-I dir]... [--no-pos] [--hash|--hash-full]
                                                      normalised dump of every export, for diffing
uscript decompile <pkg> [-I dir]... [-o outdir] [-j N] [Class]...
                                                      writes <outdir>/<Pkg>/Classes/*.uc
uscript compile <srcdir/Pkg> -o out.upk [-I dir]... [-P dep]... [flags]
uscript project init <cooked-dir> <project-dir> [-j N]
uscript project build <project-dir> [-o outdir] [-j N] [Pkg]...
```

Parallelism (`-j N`, default all cores; `USCRIPT_JOBS` env overrides the default): `decompile`
renders classes concurrently against the shared read-only context, `project init` scans package
headers concurrently (`Package::load_tables`, no content decompression), and `project build` runs
packages whose dependencies are built concurrently. A single `compile` is deliberately
single-threaded: name interning and import creation order determine the bytes, so the compiler
passes and the emitter must stay sequential. Output is identical at every `-j`; `-j 1` is the
reference when in doubt.

Compile flags:

| Flag | Meaning |
|---|---|
| `-I dir` | package search path (first match wins; list rebuilt outputs before the game dir) |
| `-P dep` | dependency package to load; an empty list means the built-in defaults, so pass `-P None` for a package with no dependencies (Core) |
| `--cooked` | cooked output: drop deprecated / non-bool editoronly fields, cooker tag nulling, etc. |
| `--strip-source` | no TextBuffer/ScriptText exports |
| `--compress` | LZO chunk layout identical to the cooker's |
| `--conform <orig>` | patch mode: seed names/imports from `<orig>`, copy every export the compile does not produce (content, ShaderCache), conform NetIndex, component maps, tags, Line/TextPos, name flags, summary |
| `--allow-config-defaults` | accept defaultproperties values for config vars |
| `--server` | server field layout (untested) |

`project init` only picks packages that define top-level classes of their own and derives dependency
order from import tables. It refuses to run if a package whose classes are imported is missing from
the directory (e.g. GFxUI for TgClient), because a missing parent package would otherwise silently
drop `extends`.

## How to add or fix a rule

1. Reproduce with the dump diff. Identify the export and token that differs.
2. Find what the engine does: read the 10897 source first. If the game clearly deviates (it does in
   places: see list below), confirm in the binary with binja rather than guessing.
3. Port the engine's logic literally, in the module that mirrors the engine function (`CompileExpr`
   → `compiler/expr.rs`, `SavePackage` sorting → `exportsort.rs`, …). Comment the engine function
   name next to the port. Do not add heuristics that merely make the current diff disappear: the
   `explicit trailing _MAX` exception and the `bool()`-stripping were both such shortcuts and both
   broke on 8.1.
4. The decompiler must print source that recompiles to the same bytes. If a cast or conversion is
   automatic in the engine's `GConversions` table (AUTOCONVERT), the decompiler strips it; otherwise
   it must be printed.
5. Run `scripts/verify.sh` on every available game version. 0 diff lines on all cooked packages, or
   explain exactly why not.
6. Record any new game-specific rule in the list below.

## Game-compiler and engine rules already ported (keep them)

Hi-Rez additions and deviations from 10897:

- State-scoped `var` (EX_StateVariable). `safeobject<Class>` property type: `cpf::SAFE_OBJECT` (bit
  46) plus Component|ExportObject for component classes, tag type `SafeObjectProperty`, passes
  uncast to typed object parameters, cast 100 to plain `Object`.
- byte locals accepted for enum parameters; int → enum-byte auto conversion (the `Enum==NULL` guard
  is absent); `exec` is not part of the override-specifier check; `client` does not imply
  `simulated`; private functions of another package only warn.
- Delegate-property tokens carry no TokenFunction (context size byte CPT_Delegate=7). `Map`
  properties get NEED_CTOR_LINK unless native.
- DominantDirectionalLightComponent / DominantSpotLightComponent non-CDO templates have a 4-byte
  native prefix.
- Explicit casts to interfaces stay DynamicCast; InterfaceCast only comes from automatic conversion
  (operator operands, array AddItem/RemoveItem/InsertItem/Find items, arguments).

Engine rules that matter for identical output:

- Header pre-scan uses `appStrfind` semantics: a keyword only matches when not preceded by a letter
  or digit (`TgInvListener_CaspianExtendStackLifetime` contains `ExtendS`).
- No conversion to bool is automatic: `bool(x)` in a condition is always explicit.
- Enum `_MAX` is exactly `GenerateEnumPrefix() + "_MAX"` (common tag prefix up to the last `_`, else
  the enum name); any other trailing `_MAX` tag is an ordinary tag (`TG_PAWN_PROPERTIES` ends
  `TGPIDX_MAX, TG_PAWN_PROPERTIES_MAX` because two misspelt `TGIPDX_` tags break the prefix).
- Cooked output drops deprecated properties and non-bool editoronly properties (fields and values);
  editoronly bools stay. Binary (immutable) struct members serialise in PropertyLink order, enum
  values as names, `ShouldSerializeValue` skips native always, transient unless serialising
  defaults, deprecated when cooked.
- `#linenumber N` is handled in the lexer; the decompiler emits it before every `assert`.
- `ignores` stubs bind through the class chain only; a stub whose super is in a state is a bodiless
  prototype.
- `Class.Enum` / `Class.Struct` through a component class adds CPF_Component|ExportObject.
- `Components.Remove(X)` records RemovedComponents; component overrides and inherited copies
  instance the template's ctor-linked sub-objects; unreferenced sub-objects are not saved; an
  explicit `Begin Object Class=X Name=N` for an already-instanced copy refines it.
- Ternary branches get the required type as hint; the decompiler keeps explicit `byte(cond ? a : b)`
  (`hint_sensitive`).
- Load flags follow NeedsLoadForClient/Server (PrimitiveComponent collision/AlwaysLoad, AnimNode
  bEditorOnly), propagated to templates; CDOs always load.
- The decompiler emits stand-ins for things the cooker removed so the chain shape is reproduced:
  `var editoronly int EditorOnlyStripped;`, `var editoronly string EditorOnlyStripped;` in structs
  that lost a ctor-link member, `EditorOnlyStripped_<Type>` locals for nulled bytecode references.

## Gotchas

- `uscript decompile` writes into `<outdir>/<Pkg>/Classes`; a stray positional argument is a class
  filter, not an output dir, and output then lands in the current directory.
- `--conform` seeds the name table with the reference's exact spellings before `None`; never reorder
  that.
- `-P` with no entries means the default dependency list, hence `-P None` for Core.
- `dump --hash` compares payload hashes; `--hash-full` prints full hex. Use `--no-pos` to ignore
  Line/TextPos.
- Nothing has been loaded in the game yet from recompiled packages; byte-identical output is the
  proxy for correctness.
