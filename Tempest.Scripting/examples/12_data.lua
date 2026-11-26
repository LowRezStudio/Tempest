-- Step 12: game data and the network. `assembly` and `lang` read the game's
-- own data files; `http` talks to web services off the game thread; `json`
-- converts both ways.

-- `champion Zhin`: the bot behind a champion name and its devices.
tempest.command("champion", function(pc, rest)
	local ok, err = assembly.load()
	if not ok then return pc:ClientMessage(err) end
	for _, bot in ipairs(assembly.find_all("BOTS", "BOT_TYPE_VALUE_ID", 10849)) do
		if (lang.message(bot.NAME_MSG_ID) or ""):lower() == rest:lower() then
			pc:ClientMessage(string.format("%s: bot %d, %s", rest, bot.BOT_ID, bot.REFERENCE_NAME))
			for _, slot in ipairs(bot.DATA_SET_BOT_DEVICES or {}) do
				local device = assembly.find("DEVICES", "DEVICE_ID", slot.DEVICE_ID)
				local name = device and lang.message(device.NAME_MSG_ID) or "?"
				pc:ClientMessage(string.format("  device %d %s", slot.DEVICE_ID, name))
			end
			return
		end
	end
	pc:ClientMessage("no champion named " .. rest)
end, "champion <name>  (assembly.dat + Lang)")

-- `http_get https://api.github.com/zen`: the callback runs on the game thread.
tempest.command("http_get", function(pc, rest)
	http.get(rest, function(res, err)
		if not res then return pc:ClientMessage("failed: " .. err) end
		pc:ClientMessage(string.format("%d %s, %d bytes", res.status, res.headers["content-type"] or "", #res.body))
		local data = res:json()
		if data then pc:ClientMessage(json.encode(data):sub(1, 200)) end
	end)
end, "http_get <url>")
