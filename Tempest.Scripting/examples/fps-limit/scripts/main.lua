-- FPS limit: a slider for Engine.MaxSmoothedFrameRate, remembered in the
-- mod's config (config.toml next to this folder for a dev mod,
-- ChaosGame/Config/mods/fps-limit.toml once installed). F8 toggles the panel,
-- `fps <n>` sets it from the console.
local cfg = tempest.config({ max_fps = 144, key = "F8" })
local open = false

local function apply()
	local engine = tempest.engine()
	if not engine then
		return false
	end
	engine.bSmoothFrameRate = true
	engine.MaxSmoothedFrameRate = cfg.max_fps
	return true
end

local function set_fps(n)
	cfg.max_fps = math.max(20, math.min(1000, math.floor(n)))
	tempest.save_config(cfg)
	if apply() then
		tempest.toast(string.format("fps limit: %d", cfg.max_fps))
	end
end

apply()

tempest.on_key(cfg.key, function()
	open = not open
	ui.capture(open)
	return true
end)

tempest.command("fps", function(pc, rest)
	local n = tonumber(rest)
	if n then
		set_fps(n)
	else
		pc:ClientMessage("fps limit: " .. cfg.max_fps .. " (fps <n> to change)")
	end
end, "fps <n>  (frame rate cap, kept in the config)")

ui.on_draw(function()
	if not open then
		return
	end
	ui.set_next_window_size(320, 0, ui.Cond.FirstUseEver)
	local still_open = ui.window("FPS limit", { open = true, flags = ui.WindowFlags.NoCollapse }, function()
		local value, changed = ui.slider_int("Max FPS", cfg.max_fps, 30, 300)
		if changed then
			cfg.max_fps = value
			apply()
		end
		-- Save once the slider is released, not on every frame of the drag.
		if ui.is_item_deactivated_after_edit() then
			tempest.save_config(cfg)
		end
		ui.same_line()
		if ui.button("Uncap") then
			set_fps(1000)
		end
		local engine = tempest.engine()
		ui.text_disabled(
			engine and string.format("engine: %.0f fps max, %.0f now", engine.MaxSmoothedFrameRate or 0, ui.framerate())
				or "no engine yet"
		)
	end)
	if still_open == false then
		open = false
		ui.capture(false)
	end
end)

print(string.format("[%s] max fps %d (%s toggles the panel)", MOD, cfg.max_fps, cfg.key))
