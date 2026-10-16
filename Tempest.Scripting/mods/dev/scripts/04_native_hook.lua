-- Step 5: native hooks and Lua console commands.
--
-- Natives need the full "Package.Class.Func" spec: the hook lives on that
-- exact UFunction. PlayerController.ConsoleCommand is native and is what the
-- in-game console (and pc:ConsoleCommand from Lua) calls; PlayerController
-- has its own override, so hooking Actor.ConsoleCommand would miss it.
tempest.before("Engine.PlayerController.ConsoleCommand", function(pc, params)
	print("ConsoleCommand: " .. tostring(params.Command))
	if params.Command == "tempest_rewrite" then
		params.Command = "stat fps"   -- edited before the native sees it
	end
end)

-- Console commands in Lua (prelude: tempest.command). Type them in the
-- game's console; "tempest_help" lists them. No cheat manager needed.
tempest.command("hello", function(pc, rest, argv)
	pc:ClientMessage("hello " .. (argv[1] or "world"))
end, "hello [name]")

tempest.command("tp", function(pc, rest, argv)
	local pawn = pc.Pawn
	if not pawn then pc:ClientMessage("no pawn") return end
	local l = pawn.Location
	pawn.Location = {
		x = l.x + (tonumber(argv[1]) or 0),
		y = l.y + (tonumber(argv[2]) or 0),
		z = l.z + (tonumber(argv[3]) or 0),
	}
end, "tp dx dy dz  (offset the pawn)")

-- A script-to-script call, visible thanks to the ProcessInternal hook:
-- Console.OutputText is UnrealScript called from other UnrealScript whenever
-- something is printed to the in-game console.
tempest.on("OutputText", function(console, params)
	print("console: " .. tostring(params.Text))
end)
