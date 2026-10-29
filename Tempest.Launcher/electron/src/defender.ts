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

// An exclusion doesn't bring back what Defender already quarantined from the folders (it often
// finishes quarantining the injector while the prompt is open), so the same elevated run restores
// those files, after waiting for detections still being remediated. Prints what it couldn't restore.
const RESTORE = `
$ErrorActionPreference = 'Continue'
function Get-Detections {
  try { $all = @(Get-MpThreatDetection) } catch { return }
  foreach ($d in $all) {
    foreach ($r in @($d.Resources | Where-Object { $_ -like 'file:_*' })) {
      $file = $r.Substring(6)
      if ($dirs | Where-Object { $file.StartsWith($_.TrimEnd('\\') + '\\', [StringComparison]::OrdinalIgnoreCase) }) {
        [pscustomobject]@{ File = $file; Status = $d.ThreatStatusID; Time = $d.InitialDetectionTime }
      }
    }
  }
}
$since = (Get-Date).AddMinutes(-5)
for ($i = 0; $i -lt 30; $i++) {
  $found = @(Get-Detections)
  if (-not ($found | Where-Object { $_.Status -eq 1 -and $_.Time -gt $since })) { break }
  Start-Sleep -Seconds 1
}
$platform = (Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows Defender' -ErrorAction SilentlyContinue).InstallLocation
$mpcmdrun = if ($platform) { Join-Path $platform 'MpCmdRun.exe' } else { "$env:ProgramFiles\\Windows Defender\\MpCmdRun.exe" }
$found | Where-Object { $_.Status -eq 3 -and -not (Test-Path -LiteralPath $_.File) } |
  Select-Object -ExpandProperty File -Unique | Where-Object {
    & $mpcmdrun -Restore -FilePath $_ | Out-Null
    -not (Test-Path -LiteralPath $_)
  }
`;

async function addExclusions(dirs: string[]): Promise<DefenderExclusionResult> {
	// An elevated process can't hand its output back to us, so its error (or the files it couldn't
	// restore) goes through a file.
	const outputLog = path.join(os.tmpdir(), `tempest-defender-${process.pid}-${Date.now()}.txt`);
	const elevated = [
		"$ErrorActionPreference = 'Stop'",
		`$dirs = @(${dirs.map(quote).join(",")})`,
		"try { Add-MpPreference -ExclusionPath $dirs } catch {",
		`  ($_ | Out-String) | Set-Content -LiteralPath ${quote(outputLog)} -Encoding UTF8`,
		"  exit 2",
		"}",
		`& { ${RESTORE} } | Set-Content -LiteralPath ${quote(outputLog)} -Encoding UTF8`,
		"exit 0",
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
	const output = (await fs.promises.readFile(outputLog, "utf8").catch(() => ""))
		.replace(/^﻿/, "")
		.trim();
	await fs.promises.rm(outputLog, { force: true }).catch(() => {});

	if (code === 0) {
		const unrestored = output ? output.split(/\r?\n/) : [];
		if (unrestored.length > 0) console.error("[defender] could not restore:", unrestored);
		return { status: "added", unrestored };
	}
	if (code === ERROR_CANCELLED) return { status: "cancelled" };
	const error = (output || stderr).trim() || `exit code ${code}`;
	console.error("[defender] adding exclusions failed:", error);
	return { status: "failed", error };
}

handle("defender:add-exclusions", (_event, { paths }) => {
	if (process.platform !== "win32") return { status: "failed", error: "Windows only" };
	const dirs = [...new Set(paths.map((p) => path.normalize(p)))].filter(isExcludable);
	if (dirs.length === 0) return { status: "failed", error: "None of the folders exist" };
	return addExclusions(dirs);
});
