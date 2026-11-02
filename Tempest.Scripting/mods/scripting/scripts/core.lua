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

-- Performance window: what every mod costs per frame (tempest.profile). Measuring only runs while the
-- window is open; closing it stops the clock.
local cfg = tempest.config({ perf_key = "Ctrl+Shift+P" })
local perf = { open = false, report = nil, next = 0 }
-- `shared` outlives a reload of this script: give back the mouse the last run took.
local held = shared["tempest-scripting.perf"] or {}
shared["tempest-scripting.perf"] = held
if held.captured then
	ui.capture(false)
	held.captured = false
	tempest.profile(false)
end

local function per_frame(r, ms)
	return r.frames > 0 and ms / r.frames or 0
end

local function lines(r)
	local out = {}
	local function add(fmt, ...)
		out[#out + 1] = string.format(fmt, ...)
	end
	local secs = math.max(r.seconds, 0.001)
	add("measured %.1f s, %d frames, %.2f ms a frame (worst %.2f ms)", r.seconds, r.frames, r.frame_ms, r.worst_frame_ms)
	for _, m in ipairs(r.mods) do
		add("%-24s %7.3f ms/frame  worst %7.3f ms  %8.0f calls/s", m.id, per_frame(r, m.ms), m.worst_frame_ms, m.calls / secs)
		for _, s in ipairs(m.sites) do
			add("    %-8s %-40s %7.3f ms/frame  max %7.3f ms  %8.0f calls/s", s.kind, s.label, per_frame(r, s.ms), s.max_ms, s.calls / secs)
		end
	end
	for _, rt in ipairs(r.runtime) do
		add("runtime  %-32s %7.3f ms/frame  max %7.3f ms  %8.0f calls/s", rt.name, per_frame(r, rt.ms), rt.max_ms, rt.calls / secs)
	end
	return out
end

local function set_perf(open)
	perf.open = open
	tempest.profile(open)
	perf.report, perf.next = nil, 0
	held.captured = open
	ui.capture(open)
end

tempest.command("tempest_perf", function(pc, rest)
	if rest == "log" then
		local r = tempest.profile()
		for _, line in ipairs(lines(r)) do
			tempest.log("[perf] " .. line)
		end
		pc:ClientMessage("performance report written to the log")
		return
	elseif rest == "reset" then
		tempest.profile("reset")
		return
	end
	set_perf(not perf.open)
	pc:ClientMessage(perf.open and "measuring: the performance window is open" or "measuring stopped")
end, "tempest_perf [log|reset]  (what every mod costs per frame)")

if cfg.perf_key ~= "" then
	local ok, err = pcall(tempest.on_key, cfg.perf_key, function()
		set_perf(not perf.open)
		return true
	end)
	if not ok then
		tempest.log("perf_key: " .. tostring(err))
	end
end

local GOOD, WARN, BAD = { 0.45, 0.85, 0.5 }, { 1, 0.78, 0.3 }, { 1, 0.45, 0.45 }

-- Share of the frame: under 1 % is noise, over 5 % worth a look.
local function share_color(pct)
	return pct < 1 and GOOD or pct < 5 and WARN or BAD
end

local function cell_ms(r, ms)
	local f = per_frame(r, ms)
	local pct = r.frame_ms > 0 and f / r.frame_ms * 100 or 0
	local c = share_color(pct)
	ui.table_next_column()
	ui.text_colored(c[1], c[2], c[3], 1, "%.3f", f)
	ui.table_next_column()
	ui.text_colored(c[1], c[2], c[3], 1, "%.1f %%", pct)
end

local function draw_perf()
	-- A fresh report twice a second: building one every frame would cost more than most mods.
	local now = os.clock()
	if not perf.report or now >= perf.next then
		perf.report, perf.next = tempest.profile(), now + 0.5
	end
	local r = perf.report
	ui.set_next_window_size(640, 420, ui.Cond.FirstUseEver)
	local still = ui.window("Tempest performance", { open = true }, function()
		local secs = math.max(r.seconds, 0.001)
		local total = 0
		for _, m in ipairs(r.mods) do
			total = total + m.ms
		end
		for _, rt in ipairs(r.runtime) do
			if rt.name ~= "UI drawing (render thread)" then
				total = total + rt.ms
			end
		end
		local f = per_frame(r, total)
		local pct = r.frame_ms > 0 and f / r.frame_ms * 100 or 0
		local c = share_color(pct)
		ui.text("Measuring for %.0f s  |  %.0f fps  |  %.2f ms a frame (worst %.1f ms)", r.seconds,
			r.frame_ms > 0 and 1000 / r.frame_ms or 0, r.frame_ms, r.worst_frame_ms)
		ui.text_colored(c[1], c[2], c[3], 1, "Scripting and Tempest take %.3f ms of each frame (%.1f %%)", f, pct)
		ui.text_disabled("Self time per frame: a mod's handlers, without other mods' hooks they set off.")
		if ui.button(r.on and "Pause" or "Resume") then
			tempest.profile(not r.on)
			perf.next = 0
		end
		ui.same_line()
		if ui.button("Reset") then
			tempest.profile("reset")
			perf.next = 0
		end
		ui.same_line()
		if ui.button("Write to log") then
			for _, line in ipairs(lines(r)) do
				tempest.log("[perf] " .. line)
			end
		end
		ui.separator()
		local flags = ui.TableFlags.RowBg + ui.TableFlags.BordersInnerV + ui.TableFlags.SizingStretchProp
		ui.table("##perf", 5, { flags = flags }, function()
			ui.table_setup_column("Mod / entry", ui.TableColumnFlags.WidthStretch, 3)
			ui.table_setup_column("ms/frame", ui.TableColumnFlags.WidthStretch, 1)
			ui.table_setup_column("% frame", ui.TableColumnFlags.WidthStretch, 1)
			ui.table_setup_column("worst / max ms", ui.TableColumnFlags.WidthStretch, 1)
			ui.table_setup_column("calls/s", ui.TableColumnFlags.WidthStretch, 1)
			ui.table_headers_row()
			for _, m in ipairs(r.mods) do
				ui.table_next_row()
				ui.table_next_column()
				local open = ui.tree_node(m.id .. "##mod", ui.TreeNodeFlags.SpanAllColumns)
				cell_ms(r, m.ms)
				ui.table_next_column()
				ui.text("%.2f", m.worst_frame_ms)
				ui.table_next_column()
				ui.text("%.0f", m.calls / secs)
				if open then
					for i, s in ipairs(m.sites) do
						ui.push_id(i)
						ui.table_next_row()
						ui.table_next_column()
						ui.text_disabled("%s", s.kind)
						ui.same_line()
						ui.text("%s", s.label ~= "" and s.label or "-")
						cell_ms(r, s.ms)
						ui.table_next_column()
						ui.text("%.2f", s.max_ms)
						ui.table_next_column()
						ui.text("%.0f", s.calls / secs)
						ui.pop_id()
					end
					ui.tree_pop()
				end
			end
			ui.table_next_row()
			ui.table_next_column()
			local open = ui.tree_node("Tempest runtime##runtime", ui.TreeNodeFlags.SpanAllColumns)
			local rt_ms = 0
			for _, rt in ipairs(r.runtime) do
				if rt.name ~= "UI drawing (render thread)" then
					rt_ms = rt_ms + rt.ms
				end
			end
			cell_ms(r, rt_ms)
			ui.table_next_column()
			ui.text("%.2f", r.runtime_worst_frame_ms)
			ui.table_next_column()
			if open then
				for i, rt in ipairs(r.runtime) do
					ui.push_id(100 + i)
					ui.table_next_row()
					ui.table_next_column()
					ui.text("%s", rt.name)
					cell_ms(r, rt.ms)
					ui.table_next_column()
					ui.text("%.2f", rt.max_ms)
					ui.table_next_column()
					ui.text("%.0f", rt.calls / secs)
					ui.pop_id()
				end
				ui.tree_pop()
			end
		end)
		ui.text_disabled("Measuring adds a little time of its own; closing this window stops it. tempest_perf log writes the table to the log.")
	end)
	if still == false then
		set_perf(false)
	end
end

ui.on_draw(function()
	if perf.open then
		draw_perf()
	end
end)
