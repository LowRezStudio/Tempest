# Tempest.Scripting — Lua modding DLL for Paladins 8.1 (UE3, Win64)

A Rust `cdylib` (`Tempest.Scripting64.dll`) injected like any other V2 mod
DLL. It hosts a LuaJIT 2.1 VM (Lua 5.1 plus the 5.2 extensions, GC64, with
`ffi`, `bit` and `jit` loaded), finds the UE3 object model through
signatures, hooks `UObject::ProcessEvent`, runs `scripts/*.lua`, and draws
Dear ImGui on the game's D3D9 or D3D11 device for the Lua `ui` module.

Deliberately small: each layer can be checked on its own before the next
one is trusted. See `examples/` for the three incremental checks.

## Build

```bash
# Host checks (macOS/Linux; no game image, all native paths report "unresolved")
cargo test -p tempest-scripting

# Win64 DLL + the four .tempest packages (local MinGW-w64, or cross/Docker as fallback)
brew install mingw-w64            # one-time (gcc + g++: ImGui is C++)
./scripts/package.sh              # -> dist/Tempest.Scripting64.dll, dist/*.tempest
```

On Windows: `.\scripts\package.ps1` (MSVC toolchain).

## Packages

Every shipped mod lives under `mods/<name>/` (a `manifest.toml` plus
`scripts/`). `scripts/package.sh` (or `just scripting-build`) stages each
one as an installable `.tempest` in `dist/`:

| package | source | id | contents |
|---------|--------|----|----------|
| `scripting.tempest` | `mods/scripting` | `tempest-scripting` | the DLL, `scripts/core.lua` (diagnostic console commands), `types/tempest.d.lua`, `.luarc.json` |
| `console.tempest` | `mods/console` | `tempest-console` | `console.lua`: enables the in-game console (F2) |
| `dev.tempest` | `mods/dev` | `tempest-dev` | the scratch mod being worked on (for iteration without packing, use the game's `mods/` folder instead, below) |
| `lib.tempest` | `mods/lib` | `tempest-scripting-lib` | example library mod exporting `greet` |

Package and folder names are free; the `id` in each `manifest.toml` is what
the launcher and the runtime key on. Only the runtime package carries the
DLL; any number of script-only mods can sit next to it.

## Install and test

Install the `.tempest` files through the launcher (or `tempest mod install`).
It injects every `dlls/*.dll` of enabled V2 mods about a second after the
game starts.

On inject the DLL opens a console and logs to `%TEMP%\tempest-scripting.log`
(fresh every run; the previous run is kept as `tempest-scripting.prev.log`).
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
| ui | `ui`, `lua` | `IDXGISwapChain::Present detour live` (or `IDirect3DDevice9::Present`), then `d3d11 backend ready` (or `d3d9`) on the first frame; the corner overlay from `11_ui.lua` shows the backend and fps; Insert opens the panel with the mouse, `ui_demo` opens ImGui's demo, `ui_status` reports frames built/rendered |
| address hooks | `hook`, `lua` | `tempest_memhook Engine.PlayerController.ConsoleCommand` in the console logs `address hook #1 on 0x..`, then one line per console command with the `this` object |
| lua natives | `hook`, `lua` | `4 dllimport function(s) bound to Lua natives` at boot (TgMod installed); `open Shootingrange_P?game=TmLuaTest.TmLuaGame` prints `LuaAdd(2, 3) = 5` and the greeting in chat, `LuaHeartbeat #n` every 5 s in the log, `luatest` round-trips from the console |

## Settings, dev mods, hot reload

The runtime reads `<game>/ChaosGame/Config/tempest.toml` (written as a
commented template on first run; `TEMPEST_SCRIPT_CONFIG=<path>` overrides):

```toml
[runtime]
console = true            # console window + Lua REPL
jit = true                # LuaJIT trace compiler
# log = "C:/temp/tempest-scripting.log"
[dev]
mods = "mods"             # <game>/mods: dev mods, hot-reloaded on save
hot_reload = true
reload_on_config = true   # hand edits of config files reload the mod (a mod's own save never does)
hot_reload_delay_ms = 300
[mods]
configs = "ChaosGame/Config/mods"   # <configs>/<mod-id>.toml, see tempest.config()
[ui]
scale = 1.0
```

`<game>/mods/<folder>/scripts/*.lua` (or `.lua` files straight in the
folder) is a mod; `manifest.toml` is optional, the folder name is the id
without one, and a dev mod with an installed mod's id replaces it. Saving a
file in a dev mod reloads that mod on the game thread (and tells the chat);
saving `<configs>/<id>.toml` reloads mod `<id>`. The runtime drops its
`types/*.d.lua` and a `.luarc.json` into `<game>/mods/` so the folder has
completion out of the box, and `tempest.dump_sdk()` writes there too.

Mod configs: `tempest.config({ speed = 1, keys = { toggle = "Insert" } })`
returns the defaults merged with the mod's shipped `config.toml` and, for
installed mods, the user's `<configs>/<id>.toml`. The editable file (a dev
mod's own `config.toml`, an installed mod's `<configs>/<id>.toml`) is created
from the merged values when missing; `tempest.save_config(tbl)` writes it.

Environment (overrides the file): `TEMPEST_SCRIPT_DIR` (single scripts
folder, replaces mod discovery), `TEMPEST_SCRIPT_LOG` (log path),
`TEMPEST_SCRIPT_CONSOLE=0` (no console window), `TEMPEST_GAME_DIR` (game
folder when the DLL is not injected from `<game>/Binaries/Win64`).

## Lua API

```lua
tempest.version
tempest.log(msg)                     -- print() goes to the same log
tempest.status()                     -- { gobjects, gnames, engine, process_event, xor_mask,
                                     --   gnames_ok, gobjects_ok, hook_installed, game_thread,
                                     --   object_count, name_count, notes = {...}, subscriptions = {[id]=spec},
                                     --   native_hooks, address_hooks, lua_natives, ui_backend, ui_ready }
tempest.on(spec, fn) -> id           -- after the original: fn(this, params)
tempest.before(spec, fn) -> id       -- before the original: edit params, `return false` to cancel
                                     -- spec: "PostLogin" (any class) or "TgGame.TgGame.PostLogin" (IsA gate)
tempest.off(id)
tempest.queue(fn)                    -- run fn on the game thread at the next event
tempest.on_game_thread()
tempest.reload(modId?)               -- re-run every mod (or just one); subscriptions/commands of the reloaded mods are dropped first
tempest.mod() / tempest.mods()       -- current mod id / the Lua mods running, in load order
tempest.installed()                  -- every mod the launcher installed, Lua or not: { id, name, version,
                                     --   authors, kind, enabled, lua, dll, files, dev, loaded, path }
tempest.config(defaults?) -> table   -- this mod's config (defaults <- <mod>/config.toml <- <configs>/<id>.toml)
tempest.save_config(tbl) -> path     -- write it back; tempest.config_path()
tempest.settings() / tempest.game_dir() -- runtime settings (read-only) / the game folder
tempest.toast(msg)                   -- log + chat message to the local player
tempest.export(name, v) / tempest.import(name) -- cross-mod values (also reachable as the `shared` table)
MOD                                  -- the running mod's id; require("a.b") loads scripts/a/b.lua of the same mod
require("ffi") / require("bit")      -- names with no file in the mod reach LuaJIT's built-in modules
tempest.objects(limit)               -- first N live objects
tempest.find_object("Class TgGame.TgGame")
tempest.find_class("TgGame.TgGame")  -- cached
tempest.engine()                     -- *GEngine
tempest.local_player()               -- Engine.GamePlayers[0].Actor, nil in menus

obj:ptr() obj:name() obj:path() obj:full_name() obj:class() obj:outer()
obj:is_a("TgGame.TgPlayerController")  obj:is_a(classObj)  obj:has("PlayerName")  obj:prop("PlayerName")  -- names are case-insensitive (FName)
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
map[key], map:get(key), map:pairs(), pairs(map), #map, map:set(key, v) -- Map proxy (set = existing keys only)
obj:map("m_Scenes", "name", "object") -- Map proxy for a `var native Map{}` (no pair types in reflection)

tempest.new("Engine.Console", outer, name?) -- construct an object (engine `new`); nil + log on failure
tempest.command(name, fn, help)      -- in-game console command: fn(pc, rest, argv); "tempest_help" lists them
tempest.commands()                   -- { {name, help}, ... }
tempest.trace(spec, n)               -- log the first n firings (object + params), then stop
tempest.on_key("Ctrl+F5", fn, opts?) / tempest.off_key(id) -- fn() on key down; return true to swallow
                                     -- opts {up=true} = on release, {repeat=true} = also auto-repeat;
                                     -- MouseLeft/Right/Middle, Mouse4/5, WheelUp/Down work as keys
tempest.spawn("Engine.PointLight", location?, rotation?, owner?, context?, { force = true }?)
                                     -- Actor.Spawn from the local player; force spawns bStatic/bNoDelete classes
tempest.default_object(class)        -- the class default object (per-class flags and defaults)
tempest.native("TmLuaTest.TmLuaGame.LuaAdd", fn) -> id -- implement a mod package's `dllimport` function:
                                     --   fn(this, params) -> return value; assign `out` params into params
                                     --   (every dllimport function is bound at startup; unimplemented = zero)
tempest.dump_sdk(dir?)               -- LuaLS stubs for every game class/struct -> <mod>/types/sdk/*.lua

ui.on_draw(fn) -> id / ui.off_draw(id) -- fn() runs every frame on the game thread (widgets only valid here)
ui.window("Title", { open = true, flags = ui.WindowFlags.NoResize }, function() ... end) -> open
ui.text("fps %.0f", ui.framerate())  ui.button("Go")  ui.checkbox("On", v) -> v, changed
ui.slider_float / slider_int / drag_float / input_text / input_int / combo / list_box / color_edit ...
                                     -- every widget returns the new value (+ changed): no pointers
ui.child / tab_bar / tab_item / tree / menu_bar / menu / popup / popup_modal / table(id, cols, fn)
ui.same_line() separator() spacing() push_color() push_var() push_id() set_next_window_pos/size()
ui.fg.line/rect/rect_filled/circle/text(...)  ui.bg.*  ui.dl.*  -- draw lists; ui.color(r,g,b,a) / ui.rgb(0xRRGGBB)
ui.add_font(path|nil, size, { merge = true, ranges = "icons" }?) -> font; ui.push_font/pop_font/with_font/set_default_font
ui.load_image(path) / ui.image_from_rgba(w, h, bytes) -> img; ui.image(img, w, h) ui.image_button(id, img) ui.fg.image(img, x1, y1, x2, y2)
ui.capture(bool) / ui.capturing()    -- give the mouse to the UI (software cursor, look input parked)
ui.want() -> mouse, keyboard, text   -- ImGui's WantCapture* flags
ui.key_pressed(ui.Key.Escape) ui.mouse() ui.screen() ui.scale(s) ui.ready() ui.backend() ui.stats()
ui.WindowFlags / Cond / Col / StyleVar / Key / TableFlags / ... -- every ImGui enum, prefix stripped
ui.C.igXxx(...)                      -- the whole cimgui API through LuaJIT's FFI (types/imgui.d.lua)

memory.base() memory.size()
memory.read_u8/u16/u32/u64/i8/i16/i32/i64/f32/f64/ptr(addr)
memory.write_u8/u16/u32/u64/i32/i64/f32/f64(addr, v)
memory.read_str(addr, max) memory.read_wstr(addr, max)
memory.scan("48 8B 05 ? ? ? ?", limit) -- absolute addresses

memory.hook(addr, fn, {args=N}) -> id -- detour any function entry: fn(original, a1..aN)
                                     --   r = original(a1, nil, x) runs the real function (nil keeps
                                     --   an argument); return a number to set the result, or
                                     --   nothing to pass through. N defaults to 4, max 8.
memory.hook(addr, fn, {args={"ptr","f32","f64"}, ret="f32"}) -- typed positions: floats come from
                                     --   XMM0..3 (stack slots past 4) and a float result goes to XMM0
memory.unhook(id)  memory.hooks()    -- { {id, addr, mod, args}, ... }
memory.call(addr, ...)               -- call any address: integer/boolean/Object/memory.f32|f64 args;
memory.call(addr, a, memory.f32(1.5), { ret = "f64" }) -- typed float result
memory.read_c(addr, "uint64_t") / memory.write_c(addr, "float", v) -- FFI-typed access (exact 64-bit as cdata)
memory.alloc(n) memory.free(p)       -- zeroed scratch buffer for pointer arguments
memory.write_str(p, s) memory.write_wstr(p, s) -- NUL-terminated ANSI / UTF-16 (TCHAR*)
tempest.object(ptr)                  -- wrap a raw UObject* argument (nil unless it looks live)
```

Unknown property reads return `nil` (logged once per call site); writes
raise a Lua error. Handler errors are logged and never reach the game.
Numbers are LuaJIT doubles: addresses and object pointers (48-bit) are
exact, but `memory.read_u64`/`read_i64` of arbitrary data round above
2^53 (use `ffi` for full 64-bit arithmetic), and `tostring` of a large
address prints in exponent form, so format them with `%x`.
Scripts run on the game thread (the boot thread queues them to the first
`ProcessEvent` after `GEngine` exists), as do hook handlers, key bindings and
console commands; calls from the console REPL are queued there automatically.
Object-table walks (`find_class`, `objects`) are only safe from that thread:
a map load grows and reallocates the table underneath anyone else.

Where hooks attach, by function kind:

| kind | entry point hooked | spec needed | notes |
|------|--------------------|-------------|-------|
| script function (any caller: C++ event or UnrealScript) | `ProcessInternal` detour | name, class optional | cancel fills the caller's return slot with `params.ReturnValue` |
| native called from UnrealScript or Lua | `UFunction.Func` swap to a runtime stub | `Package.Class.Func`, or a bare name (every native of that name, rescanned as classes load) | `out` params are captured and written back with assignment semantics (strings and arrays included) |
| native called from C++ via `ProcessEvent` | `ProcessEvent` detour | name, class optional | |
| any C++ function (not a UFunction) | inline detour on its entry (`memory.hook`) | address, from `memory.scan` or a resolved pointer | integer args/result only; handlers run on the calling thread |

**Lua-implemented natives.** A mod's UnrealScript package can declare
functions with no body for Lua to implement: `class X extends Y
DLLBind(TempestScripting);` plus `dllimport final function int Foo(int A);`
(UDK compiles this; parameters are limited to int/float/byte/bool/string
and structs of those, no object references). The Paladins engine binds such
a function to its empty script stub but, lacking `FUNC_Defined`, skips every
call to it (`CallFunction` → `SkipFunction`: zero result). At startup the
runtime sets `FUNC_Defined` on every `dllimport` function and rebinds its
`Func` to a thunk: bytecode calls then take the script path into the
`ProcessInternal` detour, engine/Lua calls reach the thunk, and either way
`tempest.native` handlers run against the live params frame, the handler's
return lands in the caller's result slot and `out` params are written back
through the frame's `OutParms` chain. Without a handler the call returns
zero and logs once. For parameter types UDK refuses on `dllimport` (object
references), declare an ordinary function with a stub body: `tempest.native`
implements it through the same handler path and the body is the fallback. See `examples/10_lua_natives.lua`
and TgMod's `TmLuaTest.TmLuaGame`.

`memory.hook` sees the Win64 integer argument registers (RCX, RDX, R8, R9)
and then the caller's stack slots, so `this` is `a1` for a method; float
arguments and results (XMM) are invisible. Several hooks on one address
chain, newest first; each `original` runs the next one. There is no limit on
how many addresses can be hooked: each one gets a small stub generated at
runtime, and the slot stays bound to its address after `unhook` (disabled,
reused by the next hook on it). The target must be a function entry point
whose first instructions the detour can relocate; otherwise `memory.hook`
raises.

Matching: a qualified spec (`Pkg.Class.Func`) binds to that exact `UFunction`
and fires whenever that function body runs (also inside a `Super` chain);
an unqualified spec matches by name (case-insensitive, like `FName`) and
fires once per logical call, at the outermost frame of a `Super` chain.
Lua-initiated calls (`obj:Func()`) bypass `ProcessEvent`-level hooks but do
pass through native thunks and `ProcessInternal`, so they are observable.

**Mods.** The DLL finds every enabled V2 mod in `<game>/.tempest/v2/mods/*/scripts/`
(enabled state from the CLI's `<game>/.tempest/mods/mods.json`, whose records
`tempest.installed()` exposes for every mod, Lua or not) and runs each in its own sandbox: globals a mod
sets are private, reads fall through to the API and standard library. A mod
needs no DLL of its own if any enabled mod ships one; if several do, the
first to load runs everything and the others stay idle. `shared` is the
one table every mod sees.

**Editor support.** `types/tempest.d.lua` describes the whole API for the
Lua Language Server; the runtime and dev mods ship it with a `.luarc.json`
(`runtime.version = "LuaJIT"`) so VS Code's
Lua extension completes `tempest.*`, `memory.*`, objects, arrays and maps.
Press F9 in game (or call `tempest.dump_sdk()`) to add stubs for every game
class with its fields and functions, generated from live reflection. API
returns are typed where the class is fixed (`tempest.local_player()` is an
`Engine.PlayerController`); hook handlers get the generic `Object`, so cast
them: `---@cast this Engine.HUD` as the first line of the handler, or
`---@type Engine.Pawn` above a local.

Built-in console commands: `tempest_help`, `tempest_reload [mod-id]`, `tempest_config`, `tempest_mods`,
`lua <code>` / `lua =<expr>` (run Lua from the game console), `ui_status`,
`ui_demo`, `ui_capture [0|1]`.

**UI.** Dear ImGui 1.91.9 is compiled into the DLL (vendored cimgui under
`crates/tempest-scripting/native/`). `overlay.rs` detours
`IDXGISwapChain::Present`/`ResizeBuffers` or `IDirect3DDevice9::Present`/`Reset`,
whichever module the game loaded (polled for up to 3 min after inject), and
the first Present initialises the matching ImGui backend. Frames are *not*
built on the render thread: each Present asks for one, the game thread
builds it at its next `ProcessEvent` (`ui.on_draw` handlers run there, so
game objects are safe to read and call), and the render thread draws a copy
of the finished draw lists. A handler that raises is logged once per
distinct message and ImGui's error recovery rebalances the window stack.
Input: the WndProc hook feeds ImGui every message; key bindings
(`tempest.on_key`) still fire while the UI holds the mouse, so a toggle key
works from inside a menu. `ui.capture(true)` draws ImGui's cursor, swallows
mouse messages (and keys while a text field is active) and calls
`IgnoreLookInput(true)` on the local player; `ui.capture(false)` undoes it.
`ui.C` exposes the complete cimgui API (`ffi.cast` on addresses the DLL
hands out, so no export table); the curated `ui.*` layer above it keeps Lua
values on both sides. Fonts added with `ui.add_font` mark the atlas dirty; the render
thread rebuilds the font texture before the next frame is built, so DX9 and
DX11 stay thread-correct. Images decode on the game thread (`stb_image`) and
upload in Present; `img:ready()` tells when. Window layouts persist in
`<game>/ChaosGame/Config/tempest-ui.ini`. `ui.scale(s)` scales the default
font and the style together. Regenerate the bindings with `scripts/gen_imgui.py` after
updating `native/cimgui/`.

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
  hooks.rs     address hooks (memory.hook: runtime stubs + detours + handler chain) and raw calls
  tls.rs       anti-cheat XOR key (WolperMod EngineTls port)
  unreal.rs    signatures, names/objects, reflection, subscriptions, hook
  lua_api.rs   tempest.* / memory.* / Object proxy
  loader.rs    scripts/ discovery (installed + dev mods), run, reload
  config.rs    tempest.toml runtime settings, per-mod config files, Lua<->TOML
  watch.rs     hot reload: polls the dev folder and the mod configs, reloads on the game thread
  overlay.rs   D3D9/D3D11 Present detours, frame cadence, Lua bridge for ui.lua
  ui.lua       the `ui` module: FFI binding of cimgui + the curated widget API
  imgui_cdef.lua, imgui_syms.rs  generated by scripts/gen_imgui.py (cdef text, enums, addresses)
crates/tempest-scripting/native/
  overlay.cpp  ImGui context, Win32/DX9/DX11 backends, vtable probes, parked frame copy
  cimgui/      vendored cimgui + Dear ImGui 1.91.9 (see VENDORED.txt); build.rs compiles it
mods/          one folder per shipped mod (manifest.toml + scripts/), packed to dist/<name>.tempest:
  scripting/   the runtime: manifest + core.lua (the DLL and types/ are added at pack time)
  console/     enables the in-game console
  lib/         tempest-scripting-lib: a DLL-less library mod
  dev/         the scratch mod: edit freely (starts as a copy of examples/)
examples/      00_status .. 11_ui: reference scripts, run by the host smoke test;
               hot-reload/ and fps-limit/ are dev mods to copy into <game>/mods
types/         tempest.d.lua (LuaLS API definitions, shipped in scripting and dev);
               types/sdk/ is the in-game `tempest_sdk` output for 8.1 (regenerate after a game update)
.luarc.json    LuaLS config for this repo: completion in examples/ and mods/ against types/
scripts/       package.sh (macOS/Linux), package.ps1 (Windows), pack.py (zip + CHECKSUMS),
               gen_imgui.py (regenerates the ImGui bindings and types/imgui.d.lua)
```

## Limits

- Paladins 8.1 Win64 client only; a new patch means re-checking the five
  signatures (start from the matching WolperMod revision).
- `TMap` cannot grow from Lua (`set` on existing keys only): inserting needs
  the engine's `TSet` rehash. Native maps (`var native Map{...}`) have no
  key/value types in reflection, so `obj:map(name, keyKind, valueKind)` names
  them; the pair layout is computed from those kinds (verified for name → object).
- No gamepad input.
- `PostLogin` and other `GameInfo` events only fire where the game is
  authoritative (listen/dedicated server).
- UI: `ui.*` widgets outside a draw handler raise (immediate mode); D3D9 is
  wired like D3D11 but has not been exercised on this client (it ships D3D11).
- `memory.hook` handlers run on the thread that called the function and
  take the Lua VM lock; hooking code that runs on the render, audio or
  async-loading threads while the game thread sits inside a Lua handler
  that waits on those threads would deadlock. Hot functions (allocator,
  per-vertex math) are a bad fit: every call round-trips through Lua.
