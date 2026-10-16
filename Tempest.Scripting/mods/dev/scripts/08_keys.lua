-- Step 8: key bindings and the SDK dump.
--   tempest.on_key("F5", fn)         fn() on key down; return true to swallow the key
--   tempest.dump_sdk()               write LuaLS stubs for every game class to <mod>/types/sdk
tempest.on_key("F5", function()
	local pc = tempest.local_player()
	if pc then pc:ClientMessage("F5 from Lua") end
	print("F5 pressed")
end)

tempest.on_key("Ctrl+F5", function()
	local pc = tempest.local_player()
	local pawn = pc and pc.Pawn
	if pawn then
		local l = pawn.Location
		pawn.Location = { x = l.x, y = l.y, z = l.z + 300 }
	end
	return true -- swallowed: the game never sees Ctrl+F5
end)

-- Release + mouse + wheel bindings.
tempest.on_key("F6", function() print("F6 released") end, { up = true })
tempest.on_key("Shift+MouseMiddle", function() print("shift + middle click") end)
tempest.on_key("WheelDown", function()
	local pc = tempest.local_player()
	if pc and pc.Pawn then print("wheel down at " .. tostring(pc.Pawn.Location.z)) end
end)

-- Spawn an actor in front of the player: `spawn Engine.KActorSpawnable` in
-- the console. The engine returns none for classes with bStatic or
-- bNoDelete (all Light classes, StaticMeshActor, Note...); the *Spawnable
-- classes exist precisely for runtime spawning.
tempest.command("spawn", function(pc, rest, argv)
	local pawn = pc.Pawn
	if not pawn or not argv[1] then pc:ClientMessage("spawn <Pkg.Class>") return end
	local l = pawn.Location
	local a = tempest.spawn(argv[1], { x = l.x + 200, y = l.y, z = l.z })
	pc:ClientMessage("spawned " .. tostring(a))
end, "spawn <Pkg.Class>  (200 units ahead on X; e.g. Engine.KActorSpawnable)")

-- F9: generate editor stubs for the whole game (takes a few seconds, once).
tempest.on_key("F9", function()
	local classes, files = tempest.dump_sdk()
	local pc = tempest.local_player()
	if pc then pc:ClientMessage(string.format("SDK: %d classes in %d files", classes, files)) end
end)
