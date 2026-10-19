; Removes a previously installed Tauri MSI build ("Tempest Launcher") so the
; Electron NSIS installer doesn't leave two installs behind. Matches MSI
; product entries (PSChildName "{GUID}") by display name in both registry
; views: HKLM (per-machine, needs a UAC prompt) and HKCU (per-user, silent).
; App data is intentionally preserved so settings carry over.
!macro customInit
	nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "foreach ($$k in (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object { $$_.DisplayName -like 'Tempest*' -and $$_.PSChildName -like '{*}' })) { Start-Process msiexec.exe -ArgumentList ('/x' + $$k.PSChildName), '/qn', '/norestart' -Verb RunAs -Wait -WindowStyle Hidden }; foreach ($$k in (Get-ItemProperty 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object { $$_.DisplayName -like 'Tempest*' -and $$_.PSChildName -like '{*}' })) { Start-Process msiexec.exe -ArgumentList ('/x' + $$k.PSChildName), '/qn', '/norestart' -Wait -WindowStyle Hidden }"`
!macroend
