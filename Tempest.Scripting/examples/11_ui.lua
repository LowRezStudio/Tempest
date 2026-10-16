-- Step 11: the in-game UI (Dear ImGui). A draw handler runs every frame on
-- the game thread; `ui.*` widgets return Lua values. Insert toggles the
-- panel and gives the mouse to the UI (ui.capture) while it is open; the
-- `ui_demo` console command opens ImGui's own demo window.
local state = {
	open = false,
	name = "Xiloe",
	volume = 0.5,
	count = 3,
	team = 1,
	tint = { 1, 0.6, 0.2, 1 },
	log = {},
}

local function note(s)
	state.log[#state.log + 1] = s
	if #state.log > 20 then table.remove(state.log, 1) end
end

tempest.on_key("Insert", function()
	state.open = not state.open
	ui.capture(state.open)
	return true
end)

ui.on_draw(function()
	-- Always-on overlay text in the corner: no window, no mouse needed.
	local w, h = ui.screen()
	ui.fg.text(8, h - 24, ui.rgb(0xffffff, 0.8), "Tempest %s | %s | %.0f fps", tempest.version, ui.backend(), ui.framerate())

	if not state.open then return end
	ui.set_next_window_size(420, 360, ui.Cond.FirstUseEver)
	ui.set_next_window_pos(w / 2, h / 2, ui.Cond.FirstUseEver, 0.5, 0.5)
	local still_open = ui.window("Tempest Dev", { open = true }, function()
		ui.tab_bar("tabs", function()
			ui.tab_item("Widgets", function()
				state.name = ui.input_text("Name", state.name)
				state.volume = ui.slider_float("Volume", state.volume, 0, 1, "%.2f")
				state.count = ui.input_int("Count", state.count)
				state.team = ui.combo("Team", state.team, { "Blue", "Red", "Spectator" })
				state.tint = ui.color_edit("Tint", state.tint)
				if ui.button("Say hello") then
					local pc = tempest.local_player()
					if pc then pc:ClientMessage("hello " .. state.name) end
					note("hello " .. state.name)
				end
				ui.same_line()
				if ui.button("Spawn light") then
					local a = tempest.spawn("Engine.KActorSpawnable")
					note("spawned " .. tostring(a))
				end
				ui.set_tooltip("Actor.Spawn from the local player")
			end)
			ui.tab_item("Game", function()
				local pc = tempest.local_player()
				if not pc then
					ui.text_disabled("no local player (not in a match)")
					return
				end
				ui.table("game", 2, { flags = ui.TableFlags.Borders + ui.TableFlags.RowBg }, function()
					local function row(k, v)
						ui.table_next_row()
						ui.table_next_column() ui.text(k)
						ui.table_next_column() ui.text(tostring(v))
					end
					row("controller", pc:name())
					row("pawn", pc.Pawn and pc.Pawn:name())
					if pc.Pawn then
						local l = pc.Pawn.Location
						row("location", string.format("%.0f %.0f %.0f", l.x, l.y, l.z))
						row("health", pc.Pawn.Health)
					end
					row("objects", tempest.status().object_count)
				end)
			end)
			ui.tab_item("Log", function()
				ui.child("log", 0, 0, function()
					for _, line in ipairs(state.log) do ui.text(line) end
				end)
			end)
		end)
	end)
	if still_open == false then
		state.open = false
		ui.capture(false)
	end
end)

tempest.command("ui_panel", function(pc)
	state.open = not state.open
	ui.capture(state.open)
	pc:ClientMessage("ui panel " .. (state.open and "open" or "closed"))
end, "toggle the example panel (same as Insert)")

print("ui example: Insert (or `ui_panel`) opens the panel; `ui_demo` opens the ImGui demo; ui.ready() = " .. tostring(ui.ready()))
