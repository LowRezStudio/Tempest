# champion-viktor

Shows a custom champion (bot 2599 by default) in the in-match champion select
and switches to it when its slot is clicked. `viktor` in the console switches
directly, `viktor_status` reports what the mod found.

The bot, its Character Class item, weapon and skin must already exist in
`assembly.dat` (Studio: Assembly > New content… > Champion). The ids, name,
subtitle, icon texture and template champion are in `config.toml`
(`<game>/ChaosGame/Config/mods/champion-viktor.toml` once installed).

What it relies on:

- `UIDataManager.m_ChampionData.m_Champions` (lobby list): a `UIDataChampion`
  is constructed with `tempest.new` from the template champion's entry and
  appended. The native `m_ChampionIdToData` map is not updated, so lobby
  code that looks the bot up by id may still not find it.
- `UIHudChampionSelect` (in match): the first free `m_mcChampion[i]` button
  is filled by hand (title, subtitle, icon) and an entry appended to
  `m_AvailableChampions`; clicking it calls the native
  `TgPlayerController.SwitchChampion(bot, bodySkin, weaponSkin, voicePack)`.
- The server decides whether the switch is accepted: a bot id it does not
  list in `GET_PROFILE_CLASSES` is refused by a stock server.
