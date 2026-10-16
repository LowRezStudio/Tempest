-- Step 2: do events reach Lua? Handlers get (this, params). `params` is a
-- live view of the call's parameters (by name or position); in `tempest.on`
-- handlers it also holds `ReturnValue`. Unqualified names match on any
-- class; prefix with "Package.Class." to bind to that exact function.

-- Which objects run PostRender here? (first four firings, then stops)
tempest.trace("PostRender", 4)

-- Once the local player exists (in a match), report where the pawn is.
local seen = false
tempest.on("PostRender", function(this, params)
	local pc = tempest.local_player()
	if seen or not pc then return end
	seen = true
	print("local player: " .. tostring(pc))
	local pawn = pc.Pawn
	if pawn then
		local loc = pawn.Location
		print(string.format("  Pawn %s at %.0f %.0f %.0f", tostring(pawn), loc.x, loc.y, loc.z))
	end
end)
