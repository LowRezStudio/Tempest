-- ViktorTheGreat: a custom champion in the champion select.
--
-- How the game builds that screen (see the Studio's investigation of
-- hud.hudchampionselect): the movie holds 60 blank Button clips; the native
-- UIHudChampionSelect scene binds them to m_mcChampion[i] /
-- m_mcChampionIcon[i] / m_mcChampionTitle[i] / m_mcChampionSubtitle[i] and
-- fills one slot per entry of m_AvailableChampions, an array of
-- UIDataChampion that the client builds from the assembly and from what the
-- server says the player owns. A champion the server never lists gets no
-- slot. This mod:
--   1. appends a UIDataChampion for the custom bot to the client's champion
--      list (UIDataManager.m_ChampionData.m_Champions) so the lobby list
--      sees it,
--   2. when the in-match champion select opens, fills the first free slot
--      itself (title, subtitle, icon) and appends the entry to the scene's
--      m_AvailableChampions so the slot index matches,
--   3. switches to the bot when that slot is clicked, and from the `viktor`
--      console command (TgPlayerController.SwitchChampion is native and
--      takes the bot id directly, no UI needed).
local cfg = tempest.config({
	bot_id = 2599,
	name = "ViktorTheGreat",
	subtitle = "Custom champion",
	icon = "MissionScreen_Icons.Texture.Cassie",
	template_bot_id = 2420,
	body_skin_id = 0,
	weapon_skin_id = 0,
	voice_pack_id = 0,
})

local SCENE = "UIHudChampionSelect"
local SLOT_FIELDS = { "m_mcChampion", "m_mcChampionIcon", "m_mcChampionTitle", "m_mcChampionSubtitle" }
-- Fields copied from the template entry (UIDataChampion, TgClient).
local COPY_FIELDS = {
	"m_nLevel", "m_nArchetype", "m_nArchetypeBotId", "m_sDamage", "m_sSpecial",
	"m_nActivePassive", "m_nActiveTalent", "m_nActiveDeck", "m_ClassItem",
}

local function log(fmt, ...)
	tempest.log("[viktor] " .. string.format(fmt, ...))
end

local function switch(pc)
	if not pc then
		return false
	end
	local ok, err = pcall(function()
		pc:SwitchChampion(cfg.bot_id, cfg.body_skin_id, cfg.weapon_skin_id, cfg.voice_pack_id)
	end)
	if ok then
		tempest.toast(string.format("switching to %s (bot %d)", cfg.name, cfg.bot_id))
	else
		log("SwitchChampion failed: %s", tostring(err))
	end
	return ok
end

-- ---------------------------------------------------------------------------
-- 1. The client's champion list (lobby).
-- ---------------------------------------------------------------------------

local data_manager
local function find_data_manager()
	if data_manager then
		return data_manager
	end
	-- UIGameMoviePlayer.m_UIDataManager holds it; find the live movie player.
	for _, obj in ipairs(tempest.objects(400000)) do
		if obj:class():name() == "UIGameMoviePlayer" and obj:has("m_UIDataManager") and obj.m_UIDataManager then
			data_manager = obj.m_UIDataManager
			return data_manager
		end
	end
	return nil
end

local function has_bot(list, bot_id)
	for _, c in list:items() do
		if c and c.m_nBotId == bot_id then
			return true
		end
	end
	return false
end

local function template_of(list)
	for _, c in list:items() do
		if c and c.m_nBotId == cfg.template_bot_id then
			return c
		end
	end
	return list[1]
end

--- A UIDataChampion for the custom bot, cloned from the template entry.
local function make_entry(outer, template)
	local entry = tempest.new("TgClient.UIDataChampion", outer, cfg.name .. "_data")
	if not entry then
		return nil
	end
	entry.m_nBotId = cfg.bot_id
	entry.m_sName = cfg.name
	entry.m_sDesc = cfg.subtitle
	entry.m_sLore = ""
	if template then
		for _, f in ipairs(COPY_FIELDS) do
			local ok, v = pcall(function()
				return template[f]
			end)
			if ok and v ~= nil then
				pcall(function()
					entry[f] = v
				end)
			end
		end
	end
	return entry
end

local function register_in_lobby()
	local mgr = find_data_manager()
	if not mgr or not mgr.m_ChampionData then
		return false
	end
	local champions = mgr.m_ChampionData.m_Champions
	if has_bot(champions, cfg.bot_id) then
		return true
	end
	local entry = make_entry(mgr.m_ChampionData, template_of(champions))
	if not entry then
		log("could not construct UIDataChampion")
		return false
	end
	champions:add(entry)
	log("added %s (bot %d) to the champion list (%d entries)", cfg.name, cfg.bot_id, #champions)
	return true
end

-- ---------------------------------------------------------------------------
-- 2. The in-match champion select: fill a slot ourselves.
-- ---------------------------------------------------------------------------

local filled = {} -- scene pointer → slot index we used

local function hud_scene()
	local pc = tempest.local_player()
	local hud = pc and pc.myHUD
	if not hud or not hud:has("m_pMovie") or not hud.m_pMovie then
		return nil
	end
	local scenes = hud.m_pMovie:map("m_Scenes", "name", "object")
	return scenes and scenes:get(SCENE) or nil
end

local function fill_slot(scene)
	local list = scene.m_AvailableChampions
	if has_bot(list, cfg.bot_id) then
		return
	end
	local n = #list -- the next free button: the native code used slots 0..n-1
	local slot = scene.m_mcChampion[n + 1] -- fixed arrays are 1-based in Lua
	if not slot then
		log("no free slot (%d champions shown)", n)
		return
	end
	local entry = make_entry(scene, template_of(list))
	if not entry then
		return
	end
	list:add(entry)
	slot:SetVisible(true)
	local title = scene.m_mcChampionTitle[n + 1]
	local subtitle = scene.m_mcChampionSubtitle[n + 1]
	local icon = scene.m_mcChampionIcon[n + 1]
	if title then
		title:SetText(cfg.name)
	end
	if subtitle then
		subtitle:SetText(cfg.subtitle)
	end
	if icon then
		-- The icon clip is a tgscript TextureLoader: its "source" member names
		-- the texture ("img://<package>.<group>.<texture>").
		icon:SetString("source", "img://" .. cfg.icon)
	end
	filled[tostring(scene:ptr())] = n + 1
	log("filled slot %d of %s with %s", n, SCENE, cfg.name)
end

-- The scene has no script "opened" event; its first animation state
-- (TransitionIn) is the signal. HandleAnimState is a script event on
-- TgGfxScene, so it reaches Lua.
tempest.on(SCENE .. ".HandleAnimState", function(scene)
	local ok, err = pcall(fill_slot, scene)
	if not ok then
		log("fill_slot: %s", tostring(err))
	end
end)

-- ---------------------------------------------------------------------------
-- 3. Clicking our slot switches to the bot.
-- ---------------------------------------------------------------------------

tempest.before(SCENE .. ".Click", function(scene, params)
	local index = filled[tostring(scene:ptr())]
	if not index then
		return
	end
	local clicked = params.pObj
	local ours = scene.m_mcChampion[index]
	if clicked and ours and clicked:ptr() == ours:ptr() then
		switch(tempest.local_player())
		return false -- the native handler would look the slot up by its own list; skip it
	end
end)

-- ---------------------------------------------------------------------------
-- Console commands and the lobby registration (retried until the UI exists).
-- ---------------------------------------------------------------------------

tempest.command("viktor", function(pc)
	switch(pc)
end, "switch to " .. cfg.name .. " (bot " .. cfg.bot_id .. ")")

tempest.command("viktor_status", function(pc)
	local mgr = find_data_manager()
	local n = mgr and mgr.m_ChampionData and #mgr.m_ChampionData.m_Champions or -1
	local scene = hud_scene()
	pc:ClientMessage(string.format("[viktor] data manager %s, %d champions in the list, HUD scene %s", tostring(mgr), n, tostring(scene)))
end, "where the champion list stands")

local tries = 0
tempest.on("PostRender", function()
	if tries >= 20 then
		return
	end
	tries = tries + 1
	local ok, done = pcall(register_in_lobby)
	if ok and done then
		tries = 20
	elseif not ok then
		log("register_in_lobby: %s", tostring(done))
	end
end)
