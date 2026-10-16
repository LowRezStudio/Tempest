-- Built-in Lua layer, run before every script load (and again on reload).
-- Keep it small: anything here is API surface.

-- tempest.command(name, fn, help): an in-game console command handled in
-- Lua. Typed into the game's console (or sent via pc:ConsoleCommand), the
-- line is intercepted in PlayerController.ConsoleCommand before the engine
-- looks for exec functions, so no cheat manager is involved.
--   fn(pc, rest, argv)   rest = everything after the command, argv = split
local commands = {}
local hooked = false

local function ensure_hook()
	if hooked then return end
	hooked = true
	tempest.before("Engine.PlayerController.ConsoleCommand", function(pc, params)
		local line = params.Command
		if type(line) ~= "string" then return end
		local name, rest = line:match("^%s*(%S+)%s*(.-)%s*$")
		if not name then return end
		local cmd = commands[name:lower()]
		if not cmd then return end
		local argv = {}
		for a in rest:gmatch("%S+") do argv[#argv + 1] = a end
		local ok, err = pcall(cmd.fn, pc, rest, argv)
		if not ok then tempest.log(string.format("[%s] command %s failed: %s", cmd.mod, cmd.name, tostring(err))) end
		return false
	end)
end

function tempest.command(name, fn, help)
	assert(type(name) == "string" and type(fn) == "function", "tempest.command(name, fn, help?)")
	local owner = tempest.mod()
	local prev = commands[name:lower()]
	if prev and prev.mod ~= owner then
		tempest.log(string.format("[%s] command `%s` replaces the one from [%s]", owner, name, prev.mod))
	end
	commands[name:lower()] = { name = name, fn = fn, help = help, mod = owner }
	ensure_hook()
end

-- tempest.on_key("Ctrl+Shift+F5", fn, opts): fn() runs on key down (no
-- auto-repeat); return true to swallow the key. opts.up = true fires on
-- release, opts.repeat = true also on auto-repeat. Mouse: MouseLeft,
-- MouseRight, MouseMiddle, Mouse4, Mouse5, WheelUp, WheelDown.
-- Backed by a WndProc hook on the game window.
local VK = {
	Backspace = 0x08, Tab = 0x09, Enter = 0x0D, Escape = 0x1B, Space = 0x20,
	PageUp = 0x21, PageDown = 0x22, End = 0x23, Home = 0x24,
	Left = 0x25, Up = 0x26, Right = 0x27, Down = 0x28, Insert = 0x2D, Delete = 0x2E,
	Tilde = 0xC0, Minus = 0xBD, Equals = 0xBB, LeftBracket = 0xDB, RightBracket = 0xDD,
	Semicolon = 0xBA, Quote = 0xDE, Comma = 0xBC, Period = 0xBE, Slash = 0xBF, Backslash = 0xDC,
	CapsLock = 0x14, NumLock = 0x90, ScrollLock = 0x91, Pause = 0x13,
	MouseLeft = 1, MouseRight = 2, MouseMiddle = 4, Mouse4 = 5, Mouse5 = 6,
	WheelUp = 0xFF01, WheelDown = 0xFF02,
}
for i = 1, 24 do VK["F" .. i] = 0x6F + i end
for i = 0, 9 do VK["NumPad" .. i] = 0x60 + i; VK[tostring(i)] = 0x30 + i end
for c = 65, 90 do VK[string.char(c)] = c end
VK.Multiply, VK.Add, VK.Subtract, VK.Decimal, VK.Divide = 0x6A, 0x6B, 0x6D, 0x6E, 0x6F

local keys = {}
local next_key_id = 0

local function parse_key(spec)
	local b = { ctrl = false, shift = false, alt = false }
	for part in spec:gmatch("[^%+]+") do
		local p = part:gsub("^%s+", ""):gsub("%s+$", "")
		local l = p:lower()
		if l == "ctrl" or l == "control" then b.ctrl = true
		elseif l == "shift" then b.shift = true
		elseif l == "alt" then b.alt = true
		else
			local vk = VK[p] or VK[p:upper()] or VK[p:sub(1, 1):upper() .. p:sub(2):lower()]
			if not vk then error("unknown key '" .. p .. "' in '" .. spec .. "'", 3) end
			b.vk = vk
		end
	end
	if not b.vk then error("no key in '" .. spec .. "'", 3) end
	return b
end

function tempest.on_key(spec, fn, opts)
	assert(type(spec) == "string" and type(fn) == "function", "tempest.on_key(spec, fn, opts?)")
	local b = parse_key(spec)
	opts = opts or {}
	b.fn, b.spec, b.mod = fn, spec, tempest.mod()
	b.up, b.rep = opts.up == true, opts["repeat"] == true
	next_key_id = next_key_id + 1
	keys[next_key_id] = b
	local ok, err = pcall(tempest._install_keyhook)
	if not ok then tempest.log("on_key: " .. tostring(err) .. " (will retry on next on_key)") end
	return next_key_id
end

function tempest.off_key(id)
	keys[id] = nil
end

-- Called from the window hook: returns true when a binding swallowed the key.
function tempest._key(vk, ctrl, shift, alt, is_down, is_repeat)
	local swallow = false
	for _, b in pairs(keys) do
		if b.vk == vk and b.ctrl == ctrl and b.shift == shift and b.alt == alt
			and b.up == (not is_down) and (b.rep or not is_repeat) then
			local ok, r = pcall(b.fn)
			if not ok then tempest.log(string.format("[%s] key %s failed: %s", b.mod, b.spec, tostring(r)))
			elseif r == true then swallow = true end
		end
	end
	return swallow
end

-- tempest.spawn("Engine.PointLight", location?, rotation?, owner?, context?):
-- spawn an Actor through Actor.Spawn (tempest.new refuses Actors by engine
-- rule). `context` is the actor to spawn from (default: the local player
-- controller); location/rotation default to the context's.
function tempest.spawn(class, location, rotation, owner, context)
	local actor = context or tempest.local_player()
	if not actor then error("tempest.spawn: no actor to spawn from (not in a match?)", 2) end
	local cls = class
	if type(class) == "string" then cls = tempest.find_class(class) end
	if not cls then error("tempest.spawn: unknown class " .. tostring(class), 2) end
	return actor:Spawn(cls, owner, nil, location or actor.Location, rotation or actor.Rotation, nil, true)
end

-- tempest.native("Pkg.Class.Func", fn): implement a `dllimport` function
-- of a mod's UnrealScript package in Lua. fn(this, params) runs instead of
-- the (empty) function; its return value becomes the function's result and
-- `out` params are whatever fn assigned into `params`. Returns the hook id
-- (tempest.off removes the implementation; the function then returns zero).
function tempest.native(spec, fn)
	assert(type(spec) == "string" and type(fn) == "function", "tempest.native(spec, fn)")
	tempest._bind_native(spec)
	return tempest.before(spec, function(this, params)
		local r = fn(this, params)
		if r ~= nil then params.ReturnValue = r end
		return false
	end)
end

-- Drop everything a mod registered here (per-mod reload).
function tempest._forget(mod)
	for k, c in pairs(commands) do
		if c.mod == mod then commands[k] = nil end
	end
	for k, b in pairs(keys) do
		if b.mod == mod then keys[k] = nil end
	end
end

-- tempest.trace(spec, n): log the first n firings of an event with the
-- firing object's class and the params, then unsubscribe. Diagnostics.
function tempest.trace(spec, n)
	n = n or 5
	local id
	id = tempest.on(spec, function(this, params)
		tempest.log(string.format("trace %s: %s %s", spec, tostring(this), tostring(params)))
		n = n - 1
		if n <= 0 then tempest.off(id) end
	end)
	return id
end

function tempest.commands()
	local out = {}
	for _, c in pairs(commands) do out[#out + 1] = { name = c.name, help = c.help } end
	table.sort(out, function(a, b) return a.name < b.name end)
	return out
end

tempest.command("tempest_reload", function(pc, rest)
	tempest.queue(function() tempest.reload(rest ~= "" and rest or nil) end)
	pc:ClientMessage("reloading " .. (rest ~= "" and rest or "all mods"))
end, "tempest_reload [mod-id]  (re-run scripts)")

-- lua <code>: run a chunk from the game console ("=expr" prints the value).
tempest.command("lua", function(pc, rest)
	local src = rest:match("^=(.*)$")
	src = src and ("return " .. src) or rest
	local fn, err = load(src, "=console", "t")
	if not fn then pc:ClientMessage("lua: " .. tostring(err)) return end
	local results = table.pack(pcall(fn))
	if not results[1] then pc:ClientMessage("lua: " .. tostring(results[2])) return end
	for i = 2, results.n do pc:ClientMessage(tostring(results[i])) end
end, "lua <code>  |  lua =<expr>")

tempest.command("tempest_help", function(pc)
	for _, c in ipairs(tempest.commands()) do
		pc:ClientMessage(c.name .. (c.help and ("  -  " .. c.help) or ""))
	end
end, "list Lua console commands")
