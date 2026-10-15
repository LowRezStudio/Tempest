# Tempest.Scripting — Lua modding DLL for Paladins 8.1 (UE3, Win64)

A Rust `cdylib` (`Tempest.Scripting64.dll`) injected like any other V2 mod
DLL. It hosts a Lua 5.4 VM, finds the UE3 object model through signatures,
hooks `UObject::ProcessEvent`, and runs `scripts/*.lua`.

Deliberately small: each layer can be checked on its own before the next
one is trusted. See `examples/` for the three incremental checks.

## Build

```bash
# Host checks (macOS/Linux; no game image, all native paths report "unresolved")
cargo test -p tempest-scripting

# Win64 DLL + dist/testmod.tempest (local MinGW-w64, or cross/Docker as fallback)
brew install mingw-w64            # one-time
./scripts/package.sh              # -> dist/Tempest.Scripting64.dll, dist/testmod.tempest
```

On Windows: `.\scripts\package.ps1` (MSVC toolchain).

## Packages

`scripts/package.sh` (or `just scripting-build`) stages four installable
`.tempest` files in `dist/`:

| package | id | contents |
|---------|----|----------|
| `scripting.tempest` | `tempest-scripting` | the DLL, `scripts/core.lua` (diagnostic console commands), `types/tempest.d.lua`, `.luarc.json` |
| `console.tempest` | `tempest-console` | `console.lua`: enables the in-game console (F2) |
| `dev.tempest` | `tempest-dev` | whatever is in `dev/`: the scratch mod being worked on (`just scripting-dev <game>` repacks and reinstalls just this one, no Rust build) |
| `lib.tempest` | `tempest-scripting-lib` | example library mod exporting `greet` |

Package and folder names are free; the `id` in each `manifest.toml` is what
the launcher and the runtime key on. Only the runtime package carries the
DLL; any number of script-only mods can sit next to it.

## Install and test

`just scripting-install <game>` installs all four. The launcher injects every
`dlls/*.dll` of enabled V2 mods about a second after the game starts.

On inject the DLL opens a console and logs to `%TEMP%\tempest-scripting.log`.
The console is also a Lua REPL: type `tempest.reload()` after editing a
script, or `=tempest.status().gnames_ok` to print an expression. (A loaded
DLL stays loaded, so `tempest mod reload-dll` cannot swap the binary; a new
DLL needs a game restart.)

The boot sequence, in order, with what each step must print:

| step | log tag | expected |
|------|---------|----------|
| signatures | `resolve` | five addresses, `xor mask=0x..` |
| arrays live | `resolve` | `GNames/GObjects live` (waits up to 90 s) |
| hook | `hook` | `ProcessEvent detour live at 0x..` |
| engine loaded | `resolve` | `GEngine live; objects=.. names=..` (waits up to 120 s) |
| scripts | `loader`, `lua` | `4 mod(s): tempest-console, tempest-scripting, tempest-scripting-lib, tempest-dev` (dev last: its manifest says `after = ["tempest-scripting-lib"]`); `00_status.lua` prints `ok=true` everywhere, no `NOTE:` lines |
| engine internals | `resolve`, `hook` | `GMalloc slot`, `GNatives`, `ProcessInternal at ..`, `ProcessInternal detour live` |
| events | `lua` | `first PostRender on Object(...)` once a match starts |
| calls | `lua` | `03_calls.lua` prints `GetHumanReadableName() = ...` and two chat messages appear |
| script/native hooks | `lua` | `native hook on Engine.PlayerController.ConsoleCommand`; typing `hello` in the game console answers in chat |
| console | `lua` | `console.lua` prints `console: enabled (F2 opens it)` (or `already present` with TgMod) |
| containers | `lua` | `06_containers.lua` lists key bindings (array of structs) and HUD scenes (map) |
| mods | `loader`, `lua` | `2 mod(s): tempest-scripting-lib, tempest-scripting-test`, `LIB_PRIVATE visible here? nil`; `greet` works in the console |
| keys | `input`, `lua` | `WndProc hook installed`; F5 prints a chat line, Ctrl+F5 hops the pawn, F9 writes `types/sdk/*.lua` |

Environment: `TEMPEST_SCRIPT_DIR` (scripts folder), `TEMPEST_SCRIPT_LOG`
(log path), `TEMPEST_SCRIPT_CONSOLE=0` (no console window).

## Lua API

```lua
tempest.version
tempest.log(msg)                     -- print() goes to the same log
tempest.status()                     -- { gobjects, gnames, engine, process_event, xor_mask,
                                     --   gnames_ok, gobjects_ok, hook_installed, game_thread,
                                     --   object_count, name_count, notes = {...}, subscriptions = {[id]=spec} }
tempest.on(spec, fn) -> id           -- after the original: fn(this, params)
tempest.before(spec, fn) -> id       -- before the original: edit params, `return false` to cancel
                                     -- spec: "PostLogin" (any class) or "TgGame.TgGame.PostLogin" (IsA gate)
tempest.off(id)
tempest.queue(fn)                    -- run fn on the game thread at the next event
tempest.on_game_thread()
tempest.reload(modId?)               -- re-run every mod (or just one); subscriptions/commands of the reloaded mods are dropped first
tempest.mod() / tempest.mods()       -- current mod id / all loaded mod ids
tempest.export(name, v) / tempest.import(name) -- cross-mod values (also reachable as the `shared` table)
MOD                                  -- the running mod's id; require("a.b") loads scripts/a/b.lua of the same mod
tempest.objects(limit)               -- first N live objects
tempest.find_object("Class TgGame.TgGame")
tempest.find_class("TgGame.TgGame")  -- cached
tempest.engine()                     -- *GEngine
tempest.local_player()               -- Engine.GamePlayers[0].Actor, nil in menus

obj:ptr() obj:name() obj:path() obj:full_name() obj:class() obj:outer()
obj:is_a("TgGame.TgPlayerController")  obj:is_a(classObj)  obj:has("PlayerName")  obj:prop("PlayerName")
obj.PlayerName                       -- reflection read: int/float/byte/bool/name/str/object,
                                     -- Vector {x,y,z}, Rotator {pitch,yaw,roll}; other structs, TArray
                                     -- and TMap come back as live proxies (below)
obj.PlayerName = "Kyiro"             -- reflection write (FString must fit its buffer)
obj:ClientMessage("hi")              -- call any UFunction (script or native); results = ReturnValue, out params...
obj:call("ClientMessage", "hi")      -- same, by name
obj.ClientMessage:params()           -- { {name, kind, out, ret}, ... };  obj.ClientMessage:native()

params.NewPlayer / params[1]         -- handler params by name or position; params.names, #params
params.Damage = 0                    -- writes into the live frame (before-handlers change the call)
params.ReturnValue                   -- readable after, settable in a cancelling before-handler

s.Field / s.Field = v / s:fields()   -- Struct proxy (any struct but Vector/Rotator); assign a table to set fields
arr[i], #arr, ipairs(arr), arr:items() -- Array proxy (1-based); arr[i] = v, arr:add(v), arr:remove(i), arr:clear()
map[key], map:get(key), map:pairs(), #map, map:set(key, v) -- Map proxy (set = existing keys only)
obj:map("m_Scenes", "name", "object") -- Map proxy for a `var native Map{}` (no pair types in reflection)

tempest.new("Engine.Console", outer, name?) -- construct an object (engine `new`); nil + log on failure
tempest.command(name, fn, help)      -- in-game console command: fn(pc, rest, argv); "tempest_help" lists them
tempest.commands()                   -- { {name, help}, ... }
tempest.trace(spec, n)               -- log the first n firings (object + params), then stop
tempest.on_key("Ctrl+F5", fn, opts?) / tempest.off_key(id) -- fn() on key down; return true to swallow
                                     -- opts {up=true} = on release, {repeat=true} = also auto-repeat;
                                     -- MouseLeft/Right/Middle, Mouse4/5, WheelUp/Down work as keys
tempest.spawn("Engine.PointLight", location?, rotation?, owner?, context?) -- Actor.Spawn from the local player
tempest.dump_sdk(dir?)               -- LuaLS stubs for every game class/struct -> <mod>/types/sdk/*.lua

memory.base() memory.size()
memory.read_u8/u16/u32/u64/i8/i16/i32/i64/f32/f64/ptr(addr)
memory.write_u8/u16/u32/u64/i32/i64/f32/f64(addr, v)
memory.read_str(addr, max) memory.read_wstr(addr, max)
memory.scan("48 8B 05 ? ? ? ?", limit) -- absolute addresses
```

Unknown property reads return `nil` (logged once per call site); writes
raise a Lua error. Handler errors are logged and never reach the game.
Calls from the console REPL are queued to the game thread automatically.

Where hooks attach, by function kind:

| kind | entry point hooked | spec needed | notes |
|------|--------------------|-------------|-------|
| script function (any caller: C++ event or UnrealScript) | `ProcessInternal` detour | name, class optional | cancel fills the caller's return slot with `params.ReturnValue` |
| native called from UnrealScript or Lua | `UFunction.Func` swap to a thunk | `Package.Class.Func` | `out` params are captured and written back (POD kinds); 64 hook slots |
| native called from C++ via `ProcessEvent` | `ProcessEvent` detour | name, class optional | |

Matching: a qualified spec (`Pkg.Class.Func`) binds to that exact `UFunction`
and fires whenever that function body runs (also inside a `Super` chain);
an unqualified spec matches by name (case-insensitive, like `FName`) and
fires once per logical call, at the outermost frame of a `Super` chain.
Lua-initiated calls (`obj:Func()`) bypass `ProcessEvent`-level hooks but do
pass through native thunks and `ProcessInternal`, so they are observable.

**Mods.** The DLL finds every enabled V2 mod next to its own
(`<game>/.tempest/v2/mods/*/scripts/`, enabled state from
`<game>/.tempest/mods.json`) and runs each in its own sandbox: globals a mod
sets are private, reads fall through to the API and standard library. A mod
needs no DLL of its own if any enabled mod ships one; if several do, the
first to load runs everything and the others stay idle. `shared` is the
one table every mod sees.

**Editor support.** `types/tempest.d.lua` describes the whole API for the
Lua Language Server; the test mod ships it with a `.luarc.json` so VS Code's
Lua extension completes `tempest.*`, `memory.*`, objects, arrays and maps.
Press F9 in game (or call `tempest.dump_sdk()`) to add stubs for every game
class with its fields and functions, generated from live reflection. API
returns are typed where the class is fixed (`tempest.local_player()` is an
`Engine.PlayerController`); hook handlers get the generic `Object`, so cast
them: `---@cast this Engine.HUD` as the first line of the handler, or
`---@type Engine.Pawn` above a local.

Built-in console commands: `tempest_help`, `tempest_reload [mod-id]`,
`lua <code>` / `lua =<expr>` (run Lua from the game console).

Roadmap and known gaps: `ROADMAP.md`.

## How it finds the game

No static game addresses. Five WolperMod signatures (proven on this client
build) resolve `GObjects`, `GNames`, `GEngine`, `ProcessEvent`, and the
anti-cheat XOR helper; the XOR key table is read from the main thread's TLS
block (`tls.rs`). `GNames[0] == "None"` is the live self-test.

Engine-core layouts (`UObject`, `FName`, `FNameEntry`, `TArray`,
`TAntiCheatArray`, `UField`/`UStruct`/`UProperty`) are baked into
`unreal.rs` and match both the 8.1 client SDK and the 8.2 server debug
types. Game-class field offsets are **not** baked in: they drift between
patches, so property access walks live `UProperty` reflection.

## Layout

```text
crates/tempest-scripting/src/
  lib.rs       DllMain → boot thread: log, resolve, hook, run scripts
  log.rs       file + console + OutputDebugString; panic hook; crash VEH
  memory.rs    module, safe region scan, read/write, RIP/call math
  tls.rs       anti-cheat XOR key (WolperMod EngineTls port)
  unreal.rs    signatures, names/objects, reflection, subscriptions, hook
  lua_api.rs   tempest.* / memory.* / Object proxy
  loader.rs    scripts/ discovery, run, reload
examples/      00_status .. 08_keys: reference scripts (dev/ starts as a copy)
examples/mods/ tempest-scripting-lib: a DLL-less library mod (dist/lib.tempest)
packaging/     scripting/ (runtime manifest + core.lua), console/ (console mod)
dev/           the scratch mod (dist/dev.tempest): edit freely
types/         tempest.d.lua (LuaLS API definitions, shipped in scripting and dev);
               types/sdk/ is the in-game `tempest_sdk` output for 8.1 (regenerate after a game update)
.luarc.json    LuaLS config for this repo: completion in examples/ and dev/ against types/
scripts/       package.sh (macOS/Linux), package.ps1 (Windows), pack.py (zip + CHECKSUMS)
```

## Limits

- Paladins 8.1 Win64 client only; a new patch means re-checking the five
  signatures (start from the matching WolperMod revision).
- `out` params of hooked natives are written back for POD kinds only (not
  strings/arrays); `TMap` cannot grow from Lua (`set` on existing keys only).
- `TMap` is read-only. Native maps (`var native Map{...}`) have no key/value types in
  reflection, so `obj:map(name, keyKind, valueKind)` names them; the pair layout is computed
  from those kinds (verified for name → object).
- `PostLogin` and other `GameInfo` events only fire where the game is
  authoritative (listen/dedicated server).
