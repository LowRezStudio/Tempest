// Legacy Tauri install migration (Windows only). The old build installed as
// an MSI that the new NSIS installer does not manage, so the renderer offers
// the user to remove it on first launch; these handlers find and uninstall it.
import { execFile, spawn } from "node:child_process";
import { handle } from "./ipc/handle.js";
import type { LegacyInstall, LegacyUninstallResult } from "./ipc/contract.js";

const UNINSTALL_QUERY_ROOTS = [
	"HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
	"HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
	"HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*",
];

const FIND_SCRIPT =
	`Get-ItemProperty '${UNINSTALL_QUERY_ROOTS.join("','")}' -ErrorAction SilentlyContinue | ` +
	"Where-Object { $_.DisplayName -like 'Tempest Launcher*' -and $_.PSChildName -like '{*}' } | " +
	"Select-Object PSChildName, DisplayName, InstallLocation | ConvertTo-Json -Compress";

function runPowerShell(
	script: string,
	timeout: number,
): Promise<{ err: Error | null; stdout: string }> {
	return new Promise((resolve) => {
		execFile(
			"powershell.exe",
			["-NoProfile", "-Command", script],
			{ windowsHide: true, timeout },
			(err, stdout) => resolve({ err, stdout: (stdout ?? "").trim() }),
		);
	});
}

function findLegacyInstalls(): Promise<LegacyInstall[]> {
	if (process.platform !== "win32") return Promise.resolve([]);
	return runPowerShell(FIND_SCRIPT, 20_000).then(({ stdout }) => {
		if (!stdout) return [];
		try {
			// ConvertTo-Json emits a bare object for a single match, an array otherwise.
			const parsed: unknown = JSON.parse(stdout);
			const list = Array.isArray(parsed) ? parsed : [parsed];
			return list.map((install) => {
				const record = install as { PSChildName?: string; DisplayName?: string };
				return { id: record.PSChildName ?? "", name: record.DisplayName ?? null };
			});
		} catch {
			return [];
		}
	});
}

// msiexec exit codes: 0/3010 success (3010 = reboot required), 1602 cancelled.
// Launched unelevated first; if that fails (e.g. a per-machine install that
// doesn't auto-elevate), retry through PowerShell's RunAs so UAC shows up.
function uninstallOne(guid: string): Promise<number> {
	return new Promise((resolve) => {
		spawn("msiexec.exe", ["/x", guid, "/qn", "/norestart"], { windowsHide: true })
			.on("error", () => resolve(1603))
			.on("exit", (code) => {
				if (code === 0 || code === 3010 || code === 1602) return resolve(code ?? 1603);
				execFile(
					"powershell.exe",
					[
						"-NoProfile",
						"-Command",
						// -ErrorAction Stop + explicit exit codes: powershell.exe would
						// otherwise exit 0 even when Start-Process throws (e.g. the user
						// cancelled the UAC prompt), which would report failure as success.
						`& { try { Start-Process msiexec.exe -ArgumentList '/x','${guid}','/qn','/norestart' -Verb RunAs -Wait -WindowStyle Hidden -ErrorAction Stop } catch { if ($_.Exception.Message -match 'cancel') { exit 1602 } else { exit 1603 } } }`,
					],
					{ windowsHide: true, timeout: 5 * 60_000 },
					(err) => resolve(err ? (typeof err.code === "number" ? err.code : 1603) : 0),
				);
			});
	});
}

handle("legacy:find", () => findLegacyInstalls());

handle("legacy:uninstall", async () => {
	const installs = await findLegacyInstalls();
	const results: LegacyUninstallResult[] = [];
	for (const install of installs) {
		results.push({
			id: install.id,
			name: install.name,
			code: await uninstallOne(install.id),
		});
	}
	return results;
});
