-- A library mod: no DLL of its own, just scripts. The runtime DLL from any
-- enabled mod loads every enabled mod's scripts/ in its own sandbox.
--
-- `require` is scoped to this mod's scripts/ directory.
local fmt = require("util.format")

-- Values for other mods go through `shared` (or tempest.export, which also
-- records which mod provided them). Mods load alphabetically by id, so a
-- library id should sort before its users, or users import lazily.
tempest.export("greet", function(pc, who)
	pc:ClientMessage(fmt.banner("hello " .. (who or "there")))
end)

-- A global set here stays private to this mod:
LIB_PRIVATE = "only visible inside tempest-scripting-lib"
print("[" .. MOD .. "] exported greet")
