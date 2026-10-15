-- Step 7: several mods, one runtime. Each mod's scripts run in a sandbox
-- (globals stay private), and mods exchange values through `shared`.
print(string.format("[%s] mods loaded: %s", MOD, table.concat(tempest.mods(), ", ")))
print(string.format("[%s] LIB_PRIVATE visible here? %s", MOD, tostring(LIB_PRIVATE)))

-- Imported lazily so load order does not matter.
tempest.command("greet", function(pc, rest, argv)
	local greet = tempest.import("greet")
	if greet then greet(pc, argv[1]) else pc:ClientMessage("tempest-scripting-lib is not installed") end
end, "greet [name]  (from the lib mod)")

-- `tempest.reload("tempest-scripting-lib")` reloads just that mod.
