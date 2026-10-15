-- Step 3: before/after hooks on a GameInfo event, plus a UFunction call.
--
-- `tempest.before` runs before the original: edit `params` in place, or
-- `return false` to cancel the call (set params.ReturnValue first if the
-- caller expects one). `tempest.on` runs after and sees the final state.

tempest.before("TgGame.TgGame.PostLogin", function(game, params)
	print("PostLogin (before): " .. tostring(params.NewPlayer))
	-- return false  -- would skip the game's PostLogin entirely
end)

tempest.on("TgGame.TgGame.PostLogin", function(game, params)
	local newPlayer = params.NewPlayer
	if not newPlayer:is_a("TgGame.TgPlayerController") then
		print("PostLogin: " .. tostring(newPlayer) .. " is not a TgPlayerController")
		return
	end
	local pri = newPlayer.PlayerReplicationInfo
	if pri == nil then
		print("PostLogin: no PlayerReplicationInfo yet")
		return
	end
	print("PostLogin: " .. tostring(pri.PlayerName) .. " -> KyiroLongAssNameHooyeeey")
	pri.PlayerName = "KyiroLongAssNameHooyeeey" -- in-place FString write: must fit the existing buffer

	-- Call a UFunction on the controller. obj.Func gives a bound function;
	-- call it with a colon. Return value + out params come back as results.
	if newPlayer:has("ClientMessage") then
		newPlayer:ClientMessage("Tempest.Scripting says hi")
	end
end)
