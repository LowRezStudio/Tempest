-- Enable the in-game console without TgMod.
--
-- Paladins ships FINAL_RELEASE script, where GameViewportClient.Init skips
-- creating the Console interaction. This does what TgModGameViewportClient
-- does: new a Console inside the viewport, register it as an interaction,
-- and point it at player 0. Open it with the key below.
local CONSOLE_KEY = "F2" -- any UE3 key name: Tilde, F2, ...
local TYPE_KEY = "Tab" -- single-line input bar

local function enable_console()
	local engine = tempest.engine()
	local viewport = engine and engine.GameViewport
	if not viewport then
		return false
	end
	if viewport.ViewportConsole then
		print("console: already present (" .. tostring(viewport.ViewportConsole) .. ")")
		return true
	end

	---@type Engine.Console
	local console = tempest.new("Engine.Console", viewport)
	if not console then
		return false
	end
	viewport.ViewportConsole = console
	-- Index 0: interactions get input in array order and the GFx UI one
	-- swallows keys, so the console must come first (TgMod inserts it during
	-- Init for the same reason). No SetConsoleTarget on the shipping viewport;
	-- with ConsoleTargetPlayer unset the console targets GamePlayers[0].
	viewport:InsertInteraction(console, 0)
	console.ConsoleKey = CONSOLE_KEY
	console.TypeKey = TYPE_KEY
	print("console: enabled (" .. CONSOLE_KEY .. " opens it)")
	return true
end

-- The viewport exists once the game renders; try on the first PostRender.
local id
id = tempest.on("PostRender", function()
	local ok, done = pcall(enable_console)
	if not ok then
		print("console: " .. tostring(done))
	end
	if ok and done then
		tempest.off(id)
	end
end)
