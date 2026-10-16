-- Core console commands shipped with the runtime. The essentials
-- (tempest_help, tempest_reload, lua) live in the DLL's prelude; these are
-- the diagnostics worth having in every install.

-- tempest_status: what resolved, straight into chat.
tempest.command("tempest_status", function(pc)
	local st = tempest.status()
	pc:ClientMessage(
		string.format(
			"Tempest.Scripting %s | objects %s | names %s | hooks: event=%s script=%s keys=%s",
			tempest.version,
			tostring(st.object_count),
			tostring(st.name_count),
			tostring(st.hook_installed),
			tostring(st.script_hook_installed),
			tostring(st.key_hook_installed)
		)
	)
	for _, note in ipairs(st.notes) do
		pc:ClientMessage("note: " .. note)
	end
	if #st.notes == 0 then
		pc:ClientMessage("everything resolved")
	end
end, "runtime status")

-- tempest_mods: loaded mods in load order.
tempest.command("tempest_mods", function(pc)
	pc:ClientMessage("mods: " .. table.concat(tempest.mods(), ", "))
end, "list loaded Lua mods")

-- tempest_hooks: every subscription with its owning mod.
tempest.command("tempest_hooks", function(pc)
	local subs = tempest.status().subscriptions
	local ids = {}
	for id in pairs(subs) do
		ids[#ids + 1] = id
	end
	table.sort(ids)
	for _, id in ipairs(ids) do
		pc:ClientMessage(string.format("#%d %s", id, subs[id]))
	end
	local addrs = memory.hooks()
	for _, h in ipairs(addrs) do
		pc:ClientMessage(string.format("address #%d 0x%x [%s]", h.id, h.addr, h.mod))
	end
	if #ids == 0 and #addrs == 0 then
		pc:ClientMessage("no hooks")
	end
end, "list hooks")

-- tempest_trace <spec> [n]: log the next n firings of an event.
tempest.command("tempest_trace", function(pc, rest, argv)
	if not argv[1] then
		pc:ClientMessage("tempest_trace <Func | Pkg.Class.Func> [n]")
		return
	end
	tempest.trace(argv[1], tonumber(argv[2]) or 5)
	pc:ClientMessage("tracing " .. argv[1] .. " (see the log)")
end, "tempest_trace <spec> [n]  (log the next n firings)")

-- tempest_sdk [dir]: write LuaLS stubs for every game class.
tempest.command("tempest_sdk", function(pc, rest)
	local classes, files = tempest.dump_sdk(rest ~= "" and rest or nil)
	pc:ClientMessage(string.format("SDK: %d classes in %d files", classes, files))
end, "tempest_sdk [dir]  (LuaLS stubs from live reflection)")
