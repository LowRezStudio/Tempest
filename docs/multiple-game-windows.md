# Multiple game windows

After launching a library instance, use the green **Open new window** button beside its controls to launch another session with the same game files and settings. This is available on the home screen, library cards, and instance page.

The main **Stop all** control stops all sessions of the selected instance. The running-session menu stops an individual session. Session numbers remain stable as other windows close; closing or stopping one instance does not stop another instance's sessions.

Close all sessions before changing mods, setting up, verifying, or removing their game folder. Concurrent launches and launches during a pending stop are rejected. Process reconciliation removes only the matching session. Wine/Proton cleanup is restricted to the selected process tree instead of sweeping other sessions in the same prefix.

Validate with `dotnet build Tempest.slnx`, launcher `pnpm check`, `node scripts/verify-instance-sessions.mjs`, and `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-launch-session-processes.ps1`. Fixtures test session state and real Windows process trees without launching an installed game. Actual Paladins and Wine/Proton multi-window behavior requires manual testing.
