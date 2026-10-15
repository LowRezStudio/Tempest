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
---@param name string
---@return { class: string, kind: string, offset: integer, size: integer, ext: integer, ext_name: string }|nil
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

---@return { gobjects: integer|nil, gnames: integer|nil, engine: integer|nil, process_event: integer|nil, process_internal: integer|nil, gmalloc: integer|nil, gnatives: integer|nil, xor_mask: integer|nil, gnames_ok: boolean, gobjects_ok: boolean, hook_installed: boolean, script_hook_installed: boolean, key_hook_installed: boolean, game_thread: integer|nil, object_count: integer|nil, name_count: integer|nil, notes: string[], subscriptions: table<integer, string>, native_hooks: string[], mods: string[] }
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
function tempest.spawn(class, location, rotation, owner, context) end
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
