# Independent game instances

Each library entry uses a separate game folder for installed mods and backups. If existing entries point to the same canonical folder, Tempest keeps the first entry on that folder and copies the others into new destinations. Imported game folders are preserved.

Config files remain shared through `Documents/My Games/Paladins`. An explicit `-homedir` launch argument overrides this default. Preparing instances never creates or copies config folders.

Copies are staged, reject existing destinations and links/junctions, rebase mod metadata, and roll back failed writes. Close games and finish or pause downloads before separating shared folders. A stale view must refresh before modifying a relocated instance. Core mods are no longer automatically updated throughout the library at startup.

Validate with `dotnet build Tempest.slnx`, launcher `pnpm check`, `node scripts/verify-instance-storage.mjs`, and `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-instance-copies.ps1`. Fixtures use disposable game folders. Live Paladins and Wine/Proton behavior requires manual testing.
