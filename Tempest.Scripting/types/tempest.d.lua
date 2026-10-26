---@meta
-- Tempest.Scripting API for the Lua Language Server (LuaLS / sumneko).
-- Point a mod's .luarc.json at this file (workspace.library) for completion.
-- Game classes: run `tempest.dump_sdk()` in game and add the output folder too.
--
-- Hook handlers receive the generic `Object`; tell the language server what
-- it really is to get field completion:
--   tempest.on("PostRender", function(this, params)
--       ---@cast this Engine.HUD
--       local pc = this.PlayerOwner   -- completes as Engine.PlayerController
--   end)
-- or annotate a local: `---@type Engine.Pawn` above `local pawn = pc.Pawn`.
-- Classes absent from the dump (not loaded when it ran) show as unknown types.

---@class Vector
---@field x number
---@field y number
---@field z number

---@class Rotator
---@field pitch integer
---@field yaw integer
---@field roll integer

--- Live view of a UE3 struct (anything but Vector/Rotator).
---@class Struct
---@field [string] any
local Struct = {}
---@return integer
function Struct:addr() end
---@return integer
function Struct:size() end
---@return string[]
function Struct:fields() end

--- Live view of a TArray. 1-based; `#arr` and `ipairs` work.
---@class Array<T>: { [integer]: T }
local Array = {}
---@return integer
function Array:addr() end
---@return string
function Array:kind() end
---@generic T
---@param v? T
---@return integer new_length
function Array:add(v) end
---@param i integer
function Array:remove(i) end
function Array:clear() end
---@generic T
---@return fun(): integer, T
function Array:items() end

--- Read-only view of a TMap.
---@class Map<K, V>: { [K]: V }
local Map = {}
---@return integer
function Map:addr() end
---@generic K, V
---@param key K
---@return V|nil
function Map:get(key) end
---@generic K, V
---@return fun(): K, V
function Map:pairs() end
--- Overwrite the value of an existing key (maps cannot grow from Lua).
---@generic K, V
---@param key K
---@param value V
function Map:set(key, value) end

--- A bound UFunction (`obj.Func`); call it as `obj:Func(...)`.
---@class Method
local Method = {}
---@return boolean
function Method:native() end
---@return { name: string, kind: string, out: boolean, ret: boolean }[]
function Method:params() end

--- Parameters of a hooked call: by name or 1-based position; writable.
---@class Params
---@field [string] any
---@field [integer] any
---@field names string[]
---@field ReturnValue any

--- A live UObject. Properties and UFunctions resolve through reflection:
--- `obj.Prop`, `obj.Prop = v`, `obj:Func(...)`.
---@class Object
---@field [string] any
local Object = {}
---@return integer
function Object:ptr() end
---@return string|nil
function Object:name() end
---@return string|nil   # "Outer.Name"
function Object:path() end
---@return string|nil   # "Class Outer.Name"
function Object:full_name() end
---@return Core.Class|nil
function Object:class() end
---@return Object|nil
function Object:outer() end
---@param class string|Object  # "Engine.Pawn" or a class object
---@return boolean
function Object:is_a(class) end
---@param name string
---@return boolean
function Object:has(name) end
--- `ptr`: the UProperty object. `ext`: the property class / struct / array inner / byte enum (0 for other
--- kinds), `ext_name` its name. `bitmask`: a bool property's bit within the dword at `offset`.
---@param name string
---@return { class: string, ptr: integer, kind: string, offset: integer, size: integer, ext: integer, ext_name: string?, bitmask: integer? }|nil
function Object:prop(name) end
---@param name string
---@param ... any
---@return any ...
function Object:call(name, ...) end
---@param name string      # a `var native Map{}` property
---@param key_kind? string # int|float|byte|name|str|object|struct:Pkg.Name (default "name")
---@param value_kind? string # (default "object")
---@return Map
function Object:map(name, key_kind, value_kind) end

---@alias HookHandler fun(this: Object, params: Params): boolean|nil

---@class tempest
---@field version string
tempest = {}

---@param msg string
function tempest.log(msg) end

---@return { arch: "x64"|"x86", gobjects: integer|nil, gnames: integer|nil, engine: integer|nil, process_event: integer|nil, process_internal: integer|nil, gmalloc: integer|nil, gnatives: integer|nil, xor_mask: integer|nil, gnames_ok: boolean, gobjects_ok: boolean, hook_installed: boolean, script_hook_installed: boolean, key_hook_installed: boolean, game_thread: integer|nil, object_count: integer|nil, name_count: integer|nil, notes: string[], subscriptions: table<integer, string>, native_hooks: string[], address_hooks: AddressHook[], lua_natives: string[], mods: string[] }
function tempest.status() end

--- After the original ran. `spec` = "Func" (any class, once per logical call) or "Pkg.Class.Func" (that exact function).
---@param spec string
---@param fn HookHandler
---@return integer id
function tempest.on(spec, fn) end

--- Before the original; edit `params` or return `false` to cancel.
---@param spec string
---@param fn HookHandler
---@return integer id
function tempest.before(spec, fn) end

---@param id integer
function tempest.off(id) end

--- Implement a function of a mod package in Lua: a `dllimport` declaration (DLLBind class, no body,
--- no object parameters) or a plain script function with a stub body (any types; the body is the
--- fallback). `fn` returns the function's result; assign `out` parameters into `params`.
---@param spec string   # "Package.Class.Func"
---@param fn fun(this: Object, params: Params): any
---@return integer id
function tempest.native(spec, fn) end

--- Run `fn` on the game thread at the next event.
---@param fn fun()
function tempest.queue(fn) end
---@return boolean
function tempest.on_game_thread() end

--- Re-run every mod, or one mod by id (its hooks, commands and keys are dropped first).
---@param mod_id? string
function tempest.reload(mod_id) end
---@return string
function tempest.mod() end
---@return string[]
function tempest.mods() end
---@class InstalledMod
---@field id string
---@field name string
---@field version string
---@field authors string[]
---@field kind string "V2" | "NativePackage" | "Asset" | "Voice" | "dev"
---@field enabled boolean
---@field lua boolean ships scripts/
---@field dll boolean ships dlls/
---@field files integer game files it installs (content)
---@field dev boolean from the dev folder
---@field loaded boolean its scripts are running
---@field path string?
--- Every mod the launcher installed (from its mods.json), Lua or not, plus the dev folder.
---@return InstalledMod[]
function tempest.installed() end
---@param name string
---@param value any
function tempest.export(name, value) end
---@param name string
---@return any
function tempest.import(name) end

---@param limit? integer
---@return Object[]
function tempest.objects(limit) end
---@param full_name string  # "Class TgGame.TgGame"
---@return Object|nil
function tempest.find_object(full_name) end
---@param path string       # "TgGame.TgGame"
---@return Core.Class|nil
function tempest.find_class(path) end
---@return Engine.Engine|nil
function tempest.engine() end
---@return Engine.PlayerController|nil   # the local PlayerController, nil out of a match
function tempest.local_player() end
--- Wrap a raw UObject* (e.g. an address-hook argument); nil unless it looks like a live object.
---@param ptr integer
---@return Object|nil
function tempest.object(ptr) end
---@param class string|Object
---@param outer Object|nil
---@param name? string
---@return Object|nil
function tempest.new(class, outer, name) end

--- In-game console command handled in Lua.
---@param name string
---@param fn fun(pc: Engine.PlayerController, rest: string, argv: string[])
---@param help? string
function tempest.command(name, fn, help) end
---@return { name: string, help: string|nil }[]
function tempest.commands() end

--- Key binding: "F5", "Ctrl+Shift+T", "Tilde", "MouseLeft", "WheelUp"... Return `true` to swallow.
---@param spec string
---@param fn fun(): boolean|nil
---@param opts? { up?: boolean, ["repeat"]?: boolean }  # fire on release / also on auto-repeat
---@return integer id
function tempest.on_key(spec, fn, opts) end

--- Spawn an Actor through Actor.Spawn (defaults: from the local player, at its location/rotation).
---@param class string|Core.Class
---@param location? Vector
---@param rotation? Rotator
---@param owner? Engine.Actor
---@param context? Engine.Actor
---@return Engine.Actor|nil
---@param opts? { force?: boolean } force: spawn bStatic/bNoDelete classes (flags cleared on the class default object around the call)
function tempest.spawn(class, location, rotation, owner, context, opts) end
--- The class default object (`Default__Name`), nil when the class is not loaded.
---@param class string|Object
---@return Object?
function tempest.default_object(class) end
---@param id integer
function tempest.off_key(id) end

--- Log the first `n` firings of an event (object + params), then stop.
---@param spec string
---@param n? integer
---@return integer id
function tempest.trace(spec, n) end

--- Write LuaLS stubs for every game class/struct into `dir` (default: <mod>/types/sdk).
---@param dir? string
---@return integer classes, integer files
function tempest.dump_sdk(dir) end

--- Values shared between mods (see tempest.export / tempest.import).
---@type table<string, any>
shared = {}

--- Id of the running mod (set per sandbox).
---@type string
MOD = ""

---@class memory
memory = {}
---@return integer|nil
function memory.base() end
---@return integer|nil
function memory.size() end
--- The memory.read_* functions return nil for 0 or an address that is not readable instead of crashing;
--- an unreadable non-null address also logs the mod and the Lua line (at most every 10 s per call site).
---@param addr integer
---@return integer|nil
function memory.read_u8(addr) end
---@param addr integer
---@return integer|nil
function memory.read_u16(addr) end
---@param addr integer
---@return integer|nil
function memory.read_u32(addr) end
---@param addr integer
---@return integer|nil
function memory.read_u64(addr) end
---@param addr integer
---@return integer|nil
function memory.read_i8(addr) end
---@param addr integer
---@return integer|nil
function memory.read_i16(addr) end
---@param addr integer
---@return integer|nil
function memory.read_i32(addr) end
---@param addr integer
---@return integer|nil
function memory.read_i64(addr) end
---@param addr integer
---@return number|nil
function memory.read_f32(addr) end
---@param addr integer
---@return number|nil
function memory.read_f64(addr) end
---@param addr integer
---@return integer|nil
function memory.read_ptr(addr) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_u8(addr, v) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_u16(addr, v) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_u32(addr, v) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_u64(addr, v) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_i32(addr, v) end
---@param addr integer
---@param v integer
---@return boolean
function memory.write_i64(addr, v) end
---@param addr integer
---@param v number
---@return boolean
function memory.write_f32(addr, v) end
---@param addr integer
---@param v number
---@return boolean
function memory.write_f64(addr, v) end
---@param addr integer
---@param max? integer
---@return string|nil
function memory.read_str(addr, max) end
---@param addr integer
---@param max? integer
---@return string|nil
function memory.read_wstr(addr, max) end
---@param pattern string  # "48 8B 05 ? ? ? ?"
---@param limit? integer
---@return integer[]
function memory.scan(pattern, limit) end

---@class AddressHook
---@field id integer
---@field addr integer
---@field mod string
---@field args string[]
---@field conv "cdecl"|"stdcall"|"thiscall"|"fastcall"

--- Runs the rest of the hook chain (or the real function). Nil/missing arguments keep the caller's values.
--- Only valid while the handler that received it runs.
---@alias HookOriginal fun(...: integer|boolean|Object|nil): integer

--- Detour the function at `addr` (a function entry point). The handler runs in its place with the
--- first `opts.args` arguments (default 4 integers, max 8). Return an integer to set the result,
--- return nothing to use whatever `original` returned (or pass the call through untouched).
--- Handlers run on the calling thread. Raises when the address cannot be detoured.
--- opts.args: a count of integer arguments (default 4, max 8) or a list of
--- kinds per position ("int"/"ptr", "f32", "f64"). On the Win64 client positions
--- are RCX, RDX, R8, R9 then the stack, floats at positions 0..3 come from XMM0..3.
--- On the Win32 client arguments live on the stack (an "f64" takes two of the 8 dword
--- slots) except what opts.conv puts in registers: "cdecl" (default) and "stdcall"
--- nothing, "thiscall" the first integer (`this`) in ECX, "fastcall" the first two
--- integers in ECX/EDX; every hook on one address must declare the same conv/args/ret.
--- opts.ret = "int" (default) | "f32" | "f64" types the handler's numeric return value
--- (float results go to XMM0 on Win64, ST(0) on Win32).
---@param addr integer
---@param fn fun(original: HookOriginal, ...: number): number|boolean|Object|nil
---@param opts? { args?: integer|string[], ret?: "int"|"f32"|"f64", conv?: "cdecl"|"stdcall"|"thiscall"|"fastcall" }
---@return integer id
function memory.hook(addr, fn, opts) end
---@param id integer
---@return boolean removed
function memory.unhook(id) end
---@return AddressHook[]
function memory.hooks() end
--- Call any address with up to 8 integer/boolean/Object/memory.f32/f64 arguments (8 stack dwords
--- on Win32, where an f64 takes two); integer result, or a float when the last argument is
--- { ret = "f32"|"f64" }. The same table's `conv` names the Win32 calling convention
--- ("cdecl" default, "stdcall", "thiscall": first argument is `this`, "fastcall"). nil on the host build.
---@param addr integer
---@param ... integer|boolean|Object|FloatArg|{ ret?: "int"|"f32"|"f64", conv?: "cdecl"|"stdcall"|"thiscall"|"fastcall" }|nil
---@return number|nil
function memory.call(addr, ...) end
---@class FloatArg
--- A float argument for memory.call / original(): passed in the XMM register of its position
--- on Win64, on the stack on Win32.
---@param v number
---@return FloatArg
function memory.f32(v) end
---@param v number
---@return FloatArg
function memory.f64(v) end
--- Every byte of [addr, addr+len) is readable (committed, or one of memory.alloc's buffers).
---@param addr integer
---@param len? integer
---@return boolean
function memory.readable(addr, len) end
--- FFI-typed read: memory.read_c(p, "uint64_t") (exact 64-bit as cdata), memory.read_c(p, "ImVec2"), ...; nil when unreadable.
---@param addr integer
---@param ctype string
---@return any
function memory.read_c(addr, ctype) end
---@param addr integer
---@param ctype string
---@param value any
---@return boolean
function memory.write_c(addr, ctype, value) end
--- Zeroed heap buffer (1 byte .. 16 MiB) for pointer arguments; release with memory.free.
---@param size integer
---@return integer addr
function memory.alloc(size) end
---@param addr integer
---@return boolean
function memory.free(addr) end
--- NUL-terminated ANSI string at `addr` (false when the destination is not writable).
---@param addr integer
---@param s string
---@return boolean
function memory.write_str(addr, s) end
--- NUL-terminated UTF-16 string at `addr` (what the engine's TCHAR* parameters expect).
---@param addr integer
---@param s string
---@return boolean
function memory.write_wstr(addr, s) end

--- This mod's config: `defaults`, then the mod's config.toml, then (installed
--- mods) the user's <game>/ChaosGame/Config/mods/<id>.toml. The editable file
--- is created from the merged values when missing. Values: strings, numbers,
--- booleans, tables (arrays or maps).
---@param defaults? table
---@return table
function tempest.config(defaults) end
--- Write this mod's config file (the table replaces it).
---@param config table
---@return string path
function tempest.save_config(config) end
---@return string? path of this mod's config file
function tempest.config_path() end
---@return string? the game folder
function tempest.game_dir() end
--- The runtime settings (tempest.toml), read-only.
---@return { path: string?, game_dir: string?, log: string?, console: boolean, jit: boolean, dev_mods: string?, hot_reload: boolean, reload_on_config: boolean, hot_reload_delay_ms: integer, mod_configs: string?, ui_scale: number }
function tempest.settings() end
--- Log `msg` and show it in chat when a local player exists.
---@param msg string
function tempest.toast(msg) end

-- ===========================================================================
-- ui: Dear ImGui (see types/imgui.d.lua for the raw ui.C API)
-- ===========================================================================

--- Packed ImU32 colour (ui.color / ui.rgb) for the draw-list functions.
---@alias ui.Color integer

--- Draw-list helpers: ui.fg (over everything), ui.bg (behind windows), ui.dl (current window).
---@class ui.DrawList
local DrawList = {}
---@param x1 number @param y1 number @param x2 number @param y2 number @param color ui.Color @param thickness? number
function DrawList.line(x1, y1, x2, y2, color, thickness) end
---@param x1 number @param y1 number @param x2 number @param y2 number @param color ui.Color @param rounding? number @param thickness? number @param flags? integer
function DrawList.rect(x1, y1, x2, y2, color, rounding, thickness, flags) end
---@param x1 number @param y1 number @param x2 number @param y2 number @param color ui.Color @param rounding? number @param flags? integer
function DrawList.rect_filled(x1, y1, x2, y2, color, rounding, flags) end
---@param x number @param y number @param radius number @param color ui.Color @param segments? integer @param thickness? number
function DrawList.circle(x, y, radius, color, segments, thickness) end
---@param x number @param y number @param radius number @param color ui.Color @param segments? integer
function DrawList.circle_filled(x, y, radius, color, segments) end
---@param x1 number @param y1 number @param x2 number @param y2 number @param x3 number @param y3 number @param color ui.Color @param thickness? number
function DrawList.triangle(x1, y1, x2, y2, x3, y3, color, thickness) end
---@param x1 number @param y1 number @param x2 number @param y2 number @param x3 number @param y3 number @param color ui.Color
function DrawList.triangle_filled(x1, y1, x2, y2, x3, y3, color) end
---@param x number @param y number @param color ui.Color @param text string @param ... any string.format arguments
function DrawList.text(x, y, color, text, ...) end
---@param size number @param x number @param y number @param color ui.Color @param text string @param ... any
function DrawList.text_sized(size, x, y, color, text, ...) end
---@param points number[] x1, y1, x2, y2, ... @param color ui.Color @param closed? boolean @param thickness? number
function DrawList.polyline(points, color, closed, thickness) end
---@param img ui.Image @param x1 number @param y1 number @param x2 number @param y2 number @param tint? ui.Color
function DrawList.image(img, x1, y1, x2, y2, tint) end
---@return ImDrawList
function DrawList.list() end

--- The in-game UI. Widgets are only valid inside a ui.on_draw handler (one
--- call per rendered frame, on the game thread). Enums: ui.WindowFlags,
--- ui.Cond, ui.Col, ui.StyleVar, ui.Key, ui.TableFlags, ui.InputTextFlags,
--- ui.TreeNodeFlags, ui.SelectableFlags, ui.HoveredFlags, ui.MouseButton, ...
--- (every ImGui enum, prefix stripped: ImGuiWindowFlags_NoTitleBar -> ui.WindowFlags.NoTitleBar).
---@class ui
---@field C imgui.C the raw cimgui API (LuaJIT FFI)
---@field fg ui.DrawList
---@field bg ui.DrawList
---@field dl ui.DrawList
---@field WindowFlags table<string, integer>
---@field ChildFlags table<string, integer>
---@field Cond table<string, integer>
---@field Col table<string, integer>
---@field StyleVar table<string, integer>
---@field Key table<string, integer>
---@field MouseButton table<string, integer>
---@field Dir table<string, integer>
---@field TableFlags table<string, integer>
---@field TableColumnFlags table<string, integer>
---@field TableRowFlags table<string, integer>
---@field TableBgTarget table<string, integer>
---@field InputTextFlags table<string, integer>
---@field TreeNodeFlags table<string, integer>
---@field SelectableFlags table<string, integer>
---@field ComboFlags table<string, integer>
---@field TabBarFlags table<string, integer>
---@field TabItemFlags table<string, integer>
---@field HoveredFlags table<string, integer>
---@field FocusedFlags table<string, integer>
---@field PopupFlags table<string, integer>
---@field SliderFlags table<string, integer>
---@field ColorEditFlags table<string, integer>
---@field ButtonFlags table<string, integer>
---@field DrawFlags table<string, integer>
ui = {}

---@param fn fun() runs every frame
---@return integer id
function ui.on_draw(fn) end
---@class ImFont: ffi.cdata*
---@class ui.Image
---@field id integer
---@field width integer
---@field height integer
local Image = {}
---@return boolean uploaded and drawable
function Image:ready() end
function Image:free() end
---@return integer ImTextureID (0 until uploaded)
function Image:texture() end
--- A TTF at `size` pixels (nil path = the built-in font at that size). Call at load time; usable from the next frame.
---@param path string?
---@param size number
---@param opts? { merge?: boolean, ranges?: "icons"|integer[] }
---@return ImFont?
function ui.add_font(path, size, opts) end
---@param font ImFont
function ui.set_default_font(font) end
---@param font ImFont
function ui.push_font(font) end
function ui.pop_font() end
---@param font ImFont @param fn fun()
function ui.with_font(font, fn) end
---@return number
function ui.font_size() end
--- PNG/JPEG/BMP/TGA from disk; decoded now, uploaded by the render thread.
---@param path string
---@return ui.Image?
function ui.load_image(path) end
---@param w integer @param h integer @param bytes string w*h*4 RGBA bytes
---@return ui.Image?
function ui.image_from_rgba(w, h, bytes) end
---@param img ui.Image @param w? number @param h? number @param opts? { uv?: number[] }
function ui.image(img, w, h, opts) end
---@param id string @param img ui.Image @param w? number @param h? number
---@return boolean clicked
function ui.image_button(id, img, w, h) end
---@param id integer
function ui.off_draw(id) end
---@return boolean backend initialised (a Present has happened)
function ui.ready() end
---@return string "d3d11" | "d3d9" | "" (none yet)
function ui.backend() end
---@return { built: integer, rendered: integer, vertices: integer, lists: integer }
function ui.stats() end
--- Give the mouse to the UI: ImGui draws its cursor, mouse messages stay away
--- from the game, the local player's look input is parked until capture(false).
---@param on? boolean default true
function ui.capture(on) end
---@return boolean
function ui.capturing() end
---@return boolean mouse, boolean keyboard, boolean text ImGui's WantCapture* flags
function ui.want() end
---@param s? number set when given
---@return number scale fonts and paddings (1 = default)
function ui.scale(s) end
---@return ImGuiIO
function ui.io() end
---@return ImGuiStyle
function ui.style() end
---@return number width, number height
function ui.screen() end
---@return number x, number y
function ui.mouse() end
---@return number
function ui.framerate() end
---@return number
function ui.delta_time() end
---@param key integer ui.Key.*
---@return boolean
function ui.key_down(key) end
---@param key integer ui.Key.* @param repeat_? boolean
---@return boolean
function ui.key_pressed(key, repeat_) end
---@param key integer
---@return boolean
function ui.key_released(key) end
---@param button? integer 0 left, 1 right, 2 middle
---@return boolean
function ui.mouse_down(button) end
---@param button? integer @param repeat_? boolean
---@return boolean
function ui.mouse_clicked(button, repeat_) end
---@param button? integer
---@return boolean
function ui.mouse_released(button) end
function ui.demo() end
function ui.metrics() end
---@param x number @param y number
---@return ImVec2
function ui.vec2(x, y) end
---@param x number @param y number @param z number @param w number
---@return ImVec4
function ui.vec4(x, y, z, w) end
---@param r number 0..1 @param g number @param b number @param a? number
---@return ui.Color
function ui.color(r, g, b, a) end
---@param hex integer 0xRRGGBB @param a? number 0..1
---@return ui.Color
function ui.rgb(hex, a) end

---@param title string @param open? boolean @param flags? integer
---@return boolean visible, boolean? open
function ui.begin_window(title, open, flags) end
function ui.end_window() end
--- ui.window(title, fn) or ui.window(title, { open = bool, flags = n }, fn); End is always called.
---@param title string
---@param opts fun()|{ open?: boolean, flags?: integer }
---@param fn? fun()
---@return boolean? open false when the close button was clicked
function ui.window(title, opts, fn) end
---@param id string @param w number @param h number
---@param opts fun()|{ child_flags?: integer, flags?: integer }
---@param fn? fun()
function ui.child(id, w, h, opts, fn) end
---@param x number @param y number @param cond? integer ui.Cond.* @param pivot_x? number @param pivot_y? number
function ui.set_next_window_pos(x, y, cond, pivot_x, pivot_y) end
---@param w number @param h number @param cond? integer
function ui.set_next_window_size(w, h, cond) end
---@param collapsed boolean @param cond? integer
function ui.set_next_window_collapsed(collapsed, cond) end
function ui.set_next_window_focus() end
---@param alpha number
function ui.set_next_window_bg_alpha(alpha) end
---@param w number
function ui.set_next_item_width(w) end
---@param open boolean @param cond? integer
function ui.set_next_item_open(open, cond) end
---@return number x, number y
function ui.window_pos() end
---@return number w, number h
function ui.window_size() end
---@return number
function ui.window_width() end
---@return number
function ui.window_height() end
---@param x number @param y number @param cond? integer
function ui.set_window_pos(x, y, cond) end
---@param w number @param h number @param cond? integer
function ui.set_window_size(w, h, cond) end
function ui.set_window_focus() end
---@param flags? integer
---@return boolean
function ui.is_window_hovered(flags) end
---@param flags? integer
---@return boolean
function ui.is_window_focused(flags) end
---@return boolean
function ui.is_window_collapsed() end
---@return number w, number h
function ui.content_region_avail() end
---@return number x, number y
function ui.cursor_pos() end
---@param x number @param y number
function ui.set_cursor_pos(x, y) end
---@return number x, number y
function ui.cursor_screen_pos() end
---@param x number @param y number
function ui.set_cursor_screen_pos(x, y) end
---@return number
function ui.scroll_y() end
---@param y number
function ui.set_scroll_y(y) end
---@return number
function ui.scroll_max_y() end
---@param ratio? number
function ui.set_scroll_here_y(ratio) end

function ui.separator() end
---@param label string
function ui.separator_text(label) end
---@param offset? number @param spacing? number
function ui.same_line(offset, spacing) end
function ui.new_line() end
function ui.spacing() end
---@param w number @param h number
function ui.dummy(w, h) end
---@param w? number
function ui.indent(w) end
---@param w? number
function ui.unindent(w) end
function ui.align_text_to_frame_padding() end
---@return number
function ui.text_line_height() end
---@return number
function ui.text_line_height_with_spacing() end
---@return number
function ui.frame_height() end
---@param text string
---@return number w, number h
function ui.calc_text_size(text) end
---@param fn fun()
function ui.group(fn) end
---@param id string|integer
function ui.push_id(id) end
function ui.pop_id() end
---@param w number
function ui.push_item_width(w) end
function ui.pop_item_width() end
---@param idx integer ui.Col.* @param r number @param g number @param b number @param a? number
function ui.push_color(idx, r, g, b, a) end
---@param n? integer
function ui.pop_color(n) end
---@param idx integer ui.StyleVar.* @param a number @param b? number second component for Vec2 vars
function ui.push_var(idx, a, b) end
---@param n? integer
function ui.pop_var(n) end
---@param x? number
function ui.push_text_wrap_pos(x) end
function ui.pop_text_wrap_pos() end
---@param disabled? boolean
function ui.begin_disabled(disabled) end
function ui.end_disabled() end

---@param text string @param ... any string.format arguments when given
function ui.text(text, ...) end
---@param r number @param g number @param b number @param a number @param text string @param ... any
function ui.text_colored(r, g, b, a, text, ...) end
---@param text string @param ... any
function ui.text_disabled(text, ...) end
---@param text string @param ... any
function ui.text_wrapped(text, ...) end
---@param label string @param text string @param ... any
function ui.label_text(label, text, ...) end
---@param text string @param ... any
function ui.bullet_text(text, ...) end
function ui.bullet() end

---@param label string @param w? number @param h? number
---@return boolean clicked
function ui.button(label, w, h) end
---@param label string
---@return boolean clicked
function ui.small_button(label) end
---@param id string @param w number @param h number @param flags? integer
---@return boolean clicked
function ui.invisible_button(id, w, h, flags) end
---@param id string @param dir integer ui.Dir.*
---@return boolean clicked
function ui.arrow_button(id, dir) end
---@param label string @param value boolean
---@return boolean value, boolean changed
function ui.checkbox(label, value) end
---@param label string @param active boolean
---@return boolean clicked
function ui.radio(label, active) end
---@param label string @param current integer @param value integer
---@return integer current, boolean clicked
function ui.radio_value(label, current, value) end
---@param fraction number 0..1 @param w? number @param h? number @param overlay? string
function ui.progress_bar(fraction, w, h, overlay) end
---@param label string @param selected? boolean @param flags? integer @param w? number @param h? number
---@return boolean clicked
function ui.selectable(label, selected, flags, w, h) end
---@param label string @param v number @param min number @param max number @param format? string @param flags? integer
---@return number v, boolean changed
function ui.slider_float(label, v, min, max, format, flags) end
---@param label string @param v integer @param min integer @param max integer @param format? string @param flags? integer
---@return integer v, boolean changed
function ui.slider_int(label, v, min, max, format, flags) end
---@param label string @param radians number @param min_deg? number @param max_deg? number @param format? string @param flags? integer
---@return number radians, boolean changed
function ui.slider_angle(label, radians, min_deg, max_deg, format, flags) end
---@param label string @param v number @param speed? number @param min? number @param max? number @param format? string @param flags? integer
---@return number v, boolean changed
function ui.drag_float(label, v, speed, min, max, format, flags) end
---@param label string @param v integer @param speed? number @param min? integer @param max? integer @param format? string @param flags? integer
---@return integer v, boolean changed
function ui.drag_int(label, v, speed, min, max, format, flags) end
---@param label string @param values number[] 2 to 4 @param min number @param max number @param format? string @param flags? integer
---@return number[] values, boolean changed
function ui.slider_floats(label, values, min, max, format, flags) end
---@param label string @param values number[] 2 to 4 @param speed? number @param min? number @param max? number @param format? string @param flags? integer
---@return number[] values, boolean changed
function ui.drag_floats(label, values, speed, min, max, format, flags) end
---@param label string @param values number[] 2 to 4 @param format? string @param flags? integer
---@return number[] values, boolean changed
function ui.input_floats(label, values, format, flags) end
---@param label string @param value string @param maxlen? integer default 255 @param flags? integer ui.InputTextFlags.*
---@return string value, boolean changed
function ui.input_text(label, value, maxlen, flags) end
---@param label string @param value string @param w? number @param h? number @param maxlen? integer default 4095 @param flags? integer
---@return string value, boolean changed
function ui.input_text_multiline(label, value, w, h, maxlen, flags) end
---@param label string @param hint string @param value string @param maxlen? integer @param flags? integer
---@return string value, boolean changed
function ui.input_text_with_hint(label, hint, value, maxlen, flags) end
---@param label string @param v integer @param step? integer @param step_fast? integer @param flags? integer
---@return integer v, boolean changed
function ui.input_int(label, v, step, step_fast, flags) end
---@param label string @param v number @param step? number @param step_fast? number @param format? string @param flags? integer
---@return number v, boolean changed
function ui.input_float(label, v, step, step_fast, format, flags) end
---@param label string @param index integer 1-based @param items string[] @param flags? integer
---@return integer index, boolean changed
function ui.combo(label, index, items, flags) end
---@param label string @param index integer 1-based @param items string[] @param height? integer rows
---@return integer index, boolean changed
function ui.list_box(label, index, items, height) end
---@param label string @param color number[] {r, g, b[, a]} 0..1 @param flags? integer
---@return number[] color, boolean changed
function ui.color_edit(label, color, flags) end
---@param label string @param color number[] {r, g, b[, a]} @param flags? integer
---@return number[] color, boolean changed
function ui.color_picker(label, color, flags) end
---@param id string @param r number @param g number @param b number @param a number @param flags? integer @param w? number @param h? number
---@return boolean clicked
function ui.color_button(id, r, g, b, a, flags, w, h) end

---@param label string @param flags? integer ui.TreeNodeFlags.*
---@return boolean open
function ui.collapsing_header(label, flags) end
---@param label string @param flags? integer
---@return boolean open (call ui.tree_pop when true)
function ui.tree_node(label, flags) end
function ui.tree_pop() end
---@param label string @param fn fun() @param flags? integer
---@return boolean open
function ui.tree(label, fn, flags) end
---@param id string @param fn fun() @param flags? integer
---@return boolean visible
function ui.tab_bar(id, fn, flags) end
---@param label string @param opts fun()|{ open?: boolean, flags?: integer } @param fn? fun()
---@return boolean? open, boolean selected
function ui.tab_item(label, opts, fn) end
---@param fn fun()
---@return boolean
function ui.menu_bar(fn) end
---@param fn fun()
---@return boolean
function ui.main_menu_bar(fn) end
---@param label string @param fn fun() @param enabled? boolean
---@return boolean open
function ui.menu(label, fn, enabled) end
---@param label string @param shortcut? string @param selected? boolean @param enabled? boolean
---@return boolean clicked
function ui.menu_item(label, shortcut, selected, enabled) end
---@param id string @param flags? integer
function ui.open_popup(id, flags) end
function ui.close_current_popup() end
---@param id string @param fn fun() @param flags? integer
---@return boolean open
function ui.popup(id, fn, flags) end
---@param title string @param opts fun()|{ open?: boolean, flags?: integer } @param fn? fun()
---@return boolean? open
function ui.popup_modal(title, opts, fn) end
---@param id string @param fn fun() @param flags? integer
---@return boolean open
function ui.popup_context_item(id, fn, flags) end
---@param id string @param columns integer @param opts fun()|{ flags?: integer, w?: number, h?: number, inner_width?: number } @param fn? fun()
---@return boolean visible
function ui.table(id, columns, opts, fn) end
---@param label string @param flags? integer @param width? number @param user_id? integer
function ui.table_setup_column(label, flags, width, user_id) end
---@param cols integer @param rows integer
function ui.table_setup_scroll_freeze(cols, rows) end
function ui.table_headers_row() end
---@param flags? integer @param min_height? number
function ui.table_next_row(flags, min_height) end
---@return boolean visible
function ui.table_next_column() end
---@param i integer
---@return boolean visible
function ui.table_set_column_index(i) end
---@return integer
function ui.table_column_index() end
---@return integer
function ui.table_row_index() end
---@param target integer ui.TableBgTarget.* @param color ui.Color @param column? integer
function ui.table_set_bg_color(target, color, column) end

---@param flags? integer ui.HoveredFlags.*
---@return boolean
function ui.is_item_hovered(flags) end
---@return boolean
function ui.is_item_active() end
---@return boolean
function ui.is_item_focused() end
---@param button? integer
---@return boolean
function ui.is_item_clicked(button) end
---@return boolean
function ui.is_item_visible() end
---@return boolean
function ui.is_item_edited() end
---@return boolean
function ui.is_item_activated() end
---@return boolean
function ui.is_item_deactivated() end
---@return boolean
function ui.is_item_deactivated_after_edit() end
---@return boolean
function ui.is_any_item_hovered() end
---@return boolean
function ui.is_any_item_active() end
---@return number x, number y
function ui.item_rect_min() end
---@return number x, number y
function ui.item_rect_max() end
---@return number w, number h
function ui.item_rect_size() end
function ui.set_item_default_focus() end
---@param offset? integer
function ui.set_keyboard_focus_here(offset) end
---@param text string @param ... any
function ui.set_tooltip(text, ...) end
---@param fn fun()
---@return boolean shown
function ui.tooltip(fn) end
---@param label string @param values number[] @param opts? { overlay?: string, min?: number, max?: number, w?: number, h?: number }
function ui.plot_lines(label, values, opts) end
---@param label string @param values number[] @param opts? { overlay?: string, min?: number, max?: number, w?: number, h?: number }
function ui.plot_histogram(label, values, opts) end
