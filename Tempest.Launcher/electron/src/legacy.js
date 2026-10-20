// Legacy Tauri install migration (Windows only). The old build installed as
// an MSI that the new NSIS installer does not manage, so the renderer offers
// the user to remove it on first launch; these handlers find and uninstall it.
import { execFile, spawn } from "node:child_process";
import { ipcMain } from "electron";

const UNINSTALL_QUERY_ROOTS = [
	"HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
	"HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
	"HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
];

const FIND_SCRIPT =
	`Get-ItemProperty '${UNINSTALL_QUERY_ROOTS.join("','")}' -ErrorAction SilentlyContinue | ` +
	"Where-Object { $_.DisplayName -like 'Tempest*' -and $_.PSChildName -like '{*}' } | " +
	"Select-Object PSChildName, DisplayName, InstallLocation | ConvertTo-Json -Compress";

function runPowerShell(script, timeout) {
	return new Promise((resolve) => {
		execFile(
			"powershell.exe",
			["-NoProfile", "-Command", script],
			{ windowsHide: true, timeout },
			(err, stdout) => resolve({ err, stdout: (stdout ?? "").trim() }),
		);
	});
}

function findLegacyInstalls() {
	if (process.platform !== "win32") return Promise.resolve([]);
	return runPowerShell(FIND_SCRIPT, 20_000).then(({ stdout }) => {
		if (!stdout) return [];
		try {
			// ConvertTo-Json emits a bare object for a single match, an array otherwise.
			const parsed = JSON.parse(stdout);
			return Array.isArray(parsed) ? parsed : [parsed];
		} catch {
			return [];
		}
	});
}

// msiexec exit codes: 0/3010 success (3010 = reboot required), 1602 cancelled.
// Launched unelevated first; if that fails (e.g. a per-machine install that
// doesn't auto-elevate), retry through PowerShell's RunAs so UAC shows up.
function uninstallOne(guid) {
	return new Promise((resolve) => {
		spawn("msiexec.exe", ["/x", guid, "/qn", "/norestart"], { windowsHide: true }).on(
			"exit",
			(code) => {
				if (code === 0 || code === 3010 || code === 1602) return resolve(code ?? 1603);
				execFile(
					"powershell.exe",
					[
						"-NoProfile",
						"-Command",
						`Start-Process msiexec.exe -ArgumentList '/x','${guid}','/qn','/norestart' -Verb RunAs -Wait -WindowStyle Hidden`,
					],
					{ windowsHide: true, timeout: 5 * 60_000 },
					(err) => resolve(err?.code ?? 1603),
				);
			},
		);
	});
}

ipcMain.handle("legacy:find", () => findLegacyInstalls());

ipcMain.handle("legacy:uninstall", async () => {
	const installs = await findLegacyInstalls();
	const results = [];
	for (const install of installs) {
		results.push({
			id: install.PSChildName,
			name: install.DisplayName,
			code: await uninstallOne(install.PSChildName),
		});
	}
	return results;
});
