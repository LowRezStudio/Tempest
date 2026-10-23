// Windows Defender exclusions for folders the CLI reports as blocked (`[av-blocked] <folder>` on
// stderr): the injector it extracts and mod DLLs. Adding one needs administrator rights, so the
// change runs in an elevated PowerShell behind a single UAC prompt.
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { handle } from "./ipc/handle.js";
import type { DefenderExclusionResult } from "./ipc/contract.js";

const ERROR_CANCELLED = 1223;

const quote = (value: string) => `'${value.replaceAll("'", "''")}'`;

function isExcludable(dir: string): boolean {
	if (!path.isAbsolute(dir) || path.parse(dir).root === path.normalize(dir)) return false;
	try {
		return fs.statSync(dir).isDirectory();
	} catch {
		return false;
	}
}

async function addExclusions(dirs: string[]): Promise<DefenderExclusionResult> {
	// An elevated process can't hand its output back to us, so its error goes through a file.
	const errorLog = path.join(os.tmpdir(), `tempest-defender-${process.pid}-${Date.now()}.txt`);
	const elevated = [
		"$ErrorActionPreference = 'Stop'",
		`try { Add-MpPreference -ExclusionPath @(${dirs.map(quote).join(",")}) } catch {`,
		`  ($_ | Out-String) | Set-Content -LiteralPath ${quote(errorLog)} -Encoding UTF8`,
		"  exit 2",
		"}",
	].join("\n");
	const encoded = Buffer.from(elevated, "utf16le").toString("base64");
	// ProcessStartInfo rather than Start-Process: Start-Process drops the Win32 error code, and
	// its message (the only way to tell a declined UAC prompt apart) is localised.
	const script = [
		"$si = New-Object System.Diagnostics.ProcessStartInfo 'powershell.exe'",
		"$si.Verb = 'runas'",
		"$si.UseShellExecute = $true",
		"$si.WindowStyle = 'Hidden'",
		`$si.Arguments = '-NoProfile -NonInteractive -EncodedCommand ${encoded}'`,
		"try { $p = [System.Diagnostics.Process]::Start($si) } catch {",
		"  [Console]::Error.WriteLine($_.Exception.Message)",
		"  $e = $_.Exception",
		"  while ($e -and -not ($e -is [System.ComponentModel.Win32Exception])) { $e = $e.InnerException }",
		"  if ($e) { exit $e.NativeErrorCode } else { exit 1 }",
		"}",
		"$p.WaitForExit()",
		"exit $p.ExitCode",
	].join("\n");

	const { code, stderr } = await new Promise<{ code: number | string; stderr: string }>(
		(resolve) => {
			execFile(
				"powershell.exe",
				["-NoProfile", "-NonInteractive", "-Command", script],
				{ windowsHide: true, timeout: 5 * 60_000 },
				(err, _stdout, stderr) => resolve({ code: err ? (err.code ?? 1) : 0, stderr }),
			);
		},
	);
	const elevatedError = await fs.promises.readFile(errorLog, "utf8").catch(() => "");
	await fs.promises.rm(errorLog, { force: true }).catch(() => {});

	if (code === 0) return { status: "added" };
	if (code === ERROR_CANCELLED) return { status: "cancelled" };
	const error = (elevatedError || stderr).trim() || `exit code ${code}`;
	console.error("[defender] adding exclusions failed:", error);
	return { status: "failed", error };
}

handle("defender:add-exclusions", (_event, { paths }) => {
	if (process.platform !== "win32") return { status: "failed", error: "Windows only" };
	const dirs = [...new Set(paths.map((p) => path.normalize(p)))].filter(isExcludable);
	if (dirs.length === 0) return { status: "failed", error: "None of the folders exist" };
	return addExclusions(dirs);
});
