-- Hot reload test. Copy this folder to <game>/mods/hot-reload, start the
-- game, then edit and save this file:
-- the log shows `[hot] ... changed (hot-reload)`, the chat shows
-- `hot reload: hot-reload`, and the counters below move.
--
-- `shared` survives a reload (only a full tempest_reload resets it), so it
-- counts how many times this script ran; everything else starts fresh.
shared.hot_runs = (shared.hot_runs or 0) + 1

-- Change this and save to see the overlay update.
local MESSAGE = "edit me and save"

-- Defaults <- config.toml next to scripts/ (a dev mod's editable config).
local cfg = tempest.config({ label = "default label", color = 0xffffff, size = 1.0 })

print(string.format("[%s] run #%d  label=%q  config=%s", MOD, shared.hot_runs, cfg.label, tostring(tempest.config_path())))

tempest.command("hot", function(pc)
	pc:ClientMessage(string.format("%s: run #%d, %s", MOD, shared.hot_runs, MESSAGE))
end, "hot  (prints the reload counter)")

ui.on_draw(function()
	local w, h = ui.screen()
	local y = h - 60
	ui.fg.rect_filled(8, y - 6, 360, y + 40, ui.color(0, 0, 0, 0.5), 4)
	ui.fg.text(16, y, ui.rgb(cfg.color), "%s  run #%d", MOD, shared.hot_runs)
	ui.fg.text(16, y + 18, ui.rgb(0xffffff), "%s | %s", MESSAGE, cfg.label)
end)
