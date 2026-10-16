-- Step 10: UnrealScript functions implemented in Lua.
--
-- A mod package declares them as `dllimport` functions of a `DLLBind` class
-- (no body, no C++); the runtime binds every such function to Lua at startup
-- and `tempest.native(spec, fn)` supplies the implementation:
--   fn(this, params) -> value        the return value; assign `out` params into `params`
-- Unimplemented ones return zero/empty and log once. Pairs with TgMod's
-- TmLuaTest.TmLuaGame (Development/Src/TmLuaTest/Classes/TmLuaGame.uc, its own
-- TmLuaTest.tempest mod so it installs without the rest of Tempest Core):
--   open Shootingrange_P?game=TmLuaTest.TmLuaGame
-- then watch the log and chat, and type `luatest` in the console.
local GAME = "TmLuaTest.TmLuaGame"

tempest.native(GAME .. ".LuaAdd", function(game, p)
	return p.A + p.B
end)

tempest.native(GAME .. ".LuaGreet", function(game, p)
	return "hello " .. tostring(p.Who) .. " from Lua"
end)

-- `out Vector` filled from Lua, bool result. (dllimport parameters are
-- limited to int/float/byte/bool/string/struct; objects come in as `this`.)
tempest.native(GAME .. ".LuaOnPlayerSpawned", function(game, p)
	print("LuaOnPlayerSpawned: player " .. tostring(p.PlayerId) .. " in " .. tostring(game))
	p.Boost = { x = 0, y = 0, z = 600 }
	return true
end)

-- No return value: called from the game mode's 5 s timer.
tempest.native(GAME .. ".LuaHeartbeat", function(game, p)
	print(string.format("LuaHeartbeat #%d at %.1fs on %s", p.Count, p.Elapsed, tostring(game)))
end)

-- Stub-bodied functions take any types: object in, object out. The body in
-- the .uc ("not implemented in Lua" / false) runs when these are not here.
tempest.native(GAME .. ".LuaDescribe", function(game, p)
	local a = p.A
	if not a then return "none" end
	return a:class():name() .. " " .. tostring(a:name()) .. " at " .. tostring(a.Location and a.Location.z)
end)

tempest.native(GAME .. ".LuaControllerOf", function(game, p)
	local pawn = p.P
	if not pawn or not pawn.Controller then return false end
	p.C = pawn.Controller
	return true
end)

-- Lua -> Lua-native round trip through the normal call path.
tempest.command("luatest", function(pc)
	local game = pc.WorldInfo and pc.WorldInfo.Game
	if not game or not game:is_a(GAME) then
		pc:ClientMessage("not in TmLuaGame (open Shootingrange_P?game=" .. GAME .. ")")
		return
	end
	pc:ClientMessage("LuaAdd(40, 2) = " .. tostring(game:LuaAdd(40, 2)))
	pc:ClientMessage("LuaGreet = " .. tostring(game:LuaGreet("console")))
	local ok, boost = game:LuaOnPlayerSpawned(pc.PlayerReplicationInfo.PlayerID)
	pc:ClientMessage(string.format("LuaOnPlayerSpawned = %s, boost z = %s", tostring(ok), tostring(boost and boost.z)))
	pc:ClientMessage("LuaDescribe(pc) = " .. tostring(game:LuaDescribe(pc)))
	local found, controller = game:LuaControllerOf(pc.Pawn)
	pc:ClientMessage(string.format("LuaControllerOf(pawn) = %s, %s", tostring(found), tostring(controller)))
	pc:ClientMessage("lua natives: " .. table.concat(tempest.status().lua_natives, ", "))
end, "luatest  (call the TmLuaGame Lua natives from the console)")
