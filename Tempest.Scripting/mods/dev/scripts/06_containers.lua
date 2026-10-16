-- Step 6: arrays, structs and maps are live views, not copies.
--   arr[i], #arr, ipairs(arr) / arr:items(), arr:add(v), arr:remove(i), arr:clear()
--   s.Field, s.Field = v, s:fields()        (Vector/Rotator stay plain {x,y,z} tables)
--   map[key], map:get(key), map:pairs(), #map   (read-only)
local done = false
tempest.on("PostRender", function()
	local pc = tempest.local_player()
	if done or not pc then return end
	done = true
	local hud = pc.myHUD

	-- TArray<LocalPlayer*>
	local players = tempest.engine().GamePlayers
	print("GamePlayers: " .. tostring(players) .. " first=" .. tostring(players[1]))

	-- TArray<struct KeyBind>: each element is a live struct proxy.
	local input = pc.PlayerInput
	if input then
		local binds = input.Bindings
		print(string.format("%d key bindings; first three:", #binds))
		for i, b in binds:items() do
			if i > 3 then break end
			print(string.format("  %s -> %s", tostring(b.Name), tostring(b.Command)))
		end
	end

	-- A struct property: live fields, assignable from a table.
	local pawn = pc.Pawn
	if pawn then
		print("pawn fields sample: " .. table.concat({ pawn:class():name(), tostring(pawn.Velocity) }, " "))
	end

	-- TMap<FName, UTgGfxScene*> on the HUD movie player. Native maps carry no
	-- pair types in reflection, so name them: obj:map(prop, keyKind, valueKind)
	-- (kinds: int, float, byte, name, str, object, struct:Pkg.Name; defaults
	-- name/object).
	if hud and hud:has("m_pMovie") and hud.m_pMovie then
		local scenes = hud.m_pMovie:map("m_Scenes", "name", "object")
		print(string.format("%d HUD scenes:", #scenes))
		for k, v in scenes:pairs() do
			print("  " .. tostring(k) .. " = " .. tostring(v))
		end
	end
end)
