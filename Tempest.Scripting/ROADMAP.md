# Tempest.Scripting roadmap

Status legend: `[x]` live-verified in game, `[~]` implemented, awaiting game test, `[ ]` not started.

## Phase 0 — core (done)

- [x] Signature resolution, TLS XOR decrypt, `GNames[0] == "None"` self-test
- [x] `ProcessEvent` detour, name-matched post-call events with decoded params
- [x] Reflection property reads/writes (int/float/byte/bool/name/str/object/Vector/Rotator)
- [x] Console REPL, `tempest.reload()`, file log

## Phase 1 — calls and hooks (done, live-verified)

- [x] **Call UFunctions**: `obj:Func(args...)` / `obj:call("Func", args...)`. Params frame built
  from the function's reflection (`UStruct.PropertySize` + `CPF_Parm` children), executed
  through the original `ProcessEvent` (bypasses our own hooks). Return value and `out` params
  come back as multiple results. Works for script and native functions alike, since
  `ProcessEvent` dispatches both.
- [x] **Pre-call hooks**: `tempest.before(spec, fn)`. Handler gets `(this, params)`; `params`
  is a live proxy over the params frame, so `params.NewPlayer = x` changes what the game sees.
  `return false` cancels the original call (set `params.ReturnValue` first if the caller
  expects one).
- [x] **Post-call hooks** keep `tempest.on(spec, fn)` with the same `(this, params)` shape;
  `params.ReturnValue` is readable there.
- [x] **Game-thread queue**: `tempest.queue(fn)` runs `fn` on the game thread at the next
  event. The REPL uses it automatically, so console calls into the engine are safe.

Phase 2 closed the two gaps phase 1 left: string params are engine-allocated, and hooks
reach script → script and script → native calls.

## Phase 2 — deeper hooks and engine memory (done, live-verified)

- [x] **`ProcessInternal` detour**: every script function execution (C++ events *and*
  UnrealScript → UnrealScript calls) reaches `tempest.before/on`, with cancel filling the
  caller's return slot. `ProcessInternal` is resolved at runtime as the `Func` the majority
  of `UFunction`s bind to; frame layout (`Node@0x14 Object@0x1C Code@0x24 Locals@0x2C`)
  from the 8.1 client `ProcessEvent` disassembly.
- [x] **Native hooks**: `UFunction.Func` swapped for one of 64 thunks. The thunk evaluates
  the caller's argument bytecode through `GNatives` into a frame, runs handlers (edit /
  cancel), then re-enters the native through its own `EX_NativeParm` script. Needs the full
  `Package.Class.Func` spec. Not yet: natives with `out` params.
- [x] **Engine allocator**: `GMalloc` (signature inside `appRealloc`) gives growable
  `FString` writes, engine-owned string args, and frees returned strings.
- [x] **Object construction**: `tempest.new(class, outer, name?)` runs the engine's `new`
  operator (`GNatives[EX_New]`) on a hand-built bytecode frame; opcode numbers verified from
  the 8.2 `GRegisterNative` initializer. Used by `examples/05_console.lua` to enable the
  in-game console without TgMod.
- [ ] `TArray` growth (`Add`) on top of the allocator — moved to phase 3.

## Phase 3 — containers and structs (done, live-verified)

- [x] `TArray` proxies: `#arr`, `arr[i]`, `arr[i] = v`, `ipairs`/`arr:items()`, `arr:add(v)`
  (grows through `GMalloc`), `arr:remove(i)`, `arr:clear()`.
- [x] Struct proxies via `UScriptStruct` reflection: live fields, `s:fields()`, assignment from
  a table (field by field) or from another struct of the same type (byte copy).
- [x] `TMap` read-only view (`#`, `map[key]`, `map:get`, `map:pairs()`). `UMapProperty.Key/Value`
  (`+0x98/+0xA0`) are null for `var native Map{...}` on this engine, so `obj:map(name, k, v)`
  supplies the pair kinds; pair layout computed from them (client `TMap` is the 0x48-byte
  pack(4) layout from the 8.1 SDK).
- [x] Lua calls that leave trailing optional args out now get the script defaults (hooked natives).

## Phase 4 — multi-mod runtime (done, live-verified)

- [x] Single runtime instance (`Local\TempestScripting-<pid>` mutex): the first DLL to load
  runs every enabled V2 mod's `scripts/` under `<game>/.tempest/v2/mods/*` (enabled state
  from `<game>/.tempest/mods.json`); later copies stay idle.
- [x] Per-mod sandbox: own environment table (globals private, reads fall through), `MOD`,
  `require` scoped to the mod's `scripts/`, errors and subscriptions attributed to the mod.
- [x] Shared surface: `shared` table + `tempest.export` / `tempest.import` (providers recorded
  in `shared.__providers`, lazy import logs a load-order hint).
- [x] `tempest.reload("mod-id")` drops that mod's subscriptions and commands, re-runs it.

## Phase 5 — console commands and input

- [x] `tempest.command("name", fn, "help")`: Lua-side prelude hooks
  `Engine.PlayerController.ConsoleCommand` (native) before the engine's exec lookup, so no
  cheat manager is needed (8.2 symbols: `APlayerController::ConsoleCommand` → `Player->Exec`
  → `ScriptConsoleExec` finds `FUNC_Exec` functions on PC/CheatManager/Pawn/HUD).
- [x] `tempest.on_key("Ctrl+F5", fn)` via a `WndProc` hook on the process's main window
  (found by pid, not the swap chain); dispatched on the game thread; `true` swallows the key.

## Phase 6 — editor support (done, live-verified)

- [x] `types/tempest.d.lua` (`---@meta` LuaLS annotations) for the whole API; the test mod
  ships it with a `.luarc.json`.
- [x] `tempest.dump_sdk(dir?)`: one `<Package>.lua` per package with `---@class Pkg.Class :
  Super` blocks, typed `---@field`s for properties and `fun(self, ...)` fields for functions,
  from live reflection. Bound to F9 in `08_keys.lua`.

## Deferred / not planned

- Win32 client, patches other than 8.1 (signatures are per build)
- HUD drawing (would need a D3D11 hook; out of scope for a scripting runtime)

## Remaining gaps (closed in the final iteration; [~] = built, not yet exercised in game)

- [~] Natives with `out` params: `GPropAddr` is found at runtime by a probe (one `EX_NativeParm`
  against a scratch frame, then the global that holds `Locals+Offset`); the thunk captures each
  out arg's l-value during evaluation, hands the native an `FOutParmRec` chain into its frame,
  and copies POD results back. Hooks on such natives are deferred until the probe ran.
- [x] `map:set(key, value)` for existing keys. Growing a `TMap` (TSet rehash) stays unsupported.
  `TArray:add` zero-fills, which is valid for every supported element kind.
- [x] `tempest.spawn(class, location?, rotation?, owner?, context?)` via `Actor.Spawn`.
- [x] Load order: `[scripting] after = ["other-mod"]` in `manifest.toml`, topological with
  alphabetical tie-break; missing deps and cycles are logged.
- [x] Input: key-up (`{up=true}`), auto-repeat opt-in, mouse buttons and wheel as keys.
      Gamepad input is not planned (XInput polling, no window messages).
- [x] `this` typing: cast in the script (`---@cast this Engine.HUD`); API returns are typed.
- [ ] No unload: the DLL stays hooked until the game exits (deliberately left open).

## Later

- [ ] **Development scripts folder.** A folder outside the installed mods (for example
  `<game>/.tempest/scripts-dev/<mod-id>/` or a path chosen in the launcher) that the loader treats
  as an extra mod, so a script can be edited in place and reloaded without packing and installing
  a `.tempest` file each time. Today `TEMPEST_SCRIPT_DIR` is the only equivalent, and it replaces
  the mods directory instead of adding to it.
- [ ] **Configuration file.** A `tempest-scripting.toml` next to the runtime (or exposed in the
  launcher) for: open the console window / REPL or not, log path and verbosity, the
  development scripts folder above, the default console key for the console-enabling script, and
  a per-mod enable list. Replaces the `TEMPEST_SCRIPT_*` environment variables.
