import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const exec = promisify(execFile);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function validateId(value: string): string {
	const id = value.trim().toLowerCase();
	if (!UUID.test(id)) throw new Error("The stored crash-report installation ID is invalid.");
	return id;
}

async function runPowerShell(script: string): Promise<string> {
	const { stdout } = await exec(
		path.join(
			process.env.SystemRoot ?? "C:\\Windows",
			"System32",
			"WindowsPowerShell",
			"v1.0",
			"powershell.exe",
		),
		["-NoProfile", "-NonInteractive", "-Command", script],
		{ windowsHide: true, timeout: 10_000 },
	);
	return stdout;
}

export async function windowsInstallationId(
	run: (script: string) => Promise<string> = runPowerShell,
): Promise<string> {
	// This key is deliberately separate from NSIS's install/uninstall keys and Electron userData.
	return validateId(
		await run(`
$ErrorActionPreference = 'Stop'
$key = 'HKCU:\\Software\\LowRezStudio\\TempestCrashReports'
$id = if (Test-Path -LiteralPath $key) { (Get-ItemProperty -LiteralPath $key).InstallationId }
if (-not $id) {
	$id = [Guid]::NewGuid().ToString()
	New-Item -Path $key -Force | Out-Null
	New-ItemProperty -LiteralPath $key -Name InstallationId -Value $id -PropertyType String -Force | Out-Null
}
[Console]::Write($id)
`),
	);
}

export async function fileInstallationId(file: string): Promise<string> {
	try {
		return validateId(await fs.readFile(file, "utf8"));
	} catch (error) {
		if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
	}

	await fs.mkdir(path.dirname(file), { recursive: true, mode: 0o700 });
	try {
		await fs.writeFile(file, randomUUID(), { flag: "wx", mode: 0o600 });
	} catch (error) {
		if (!(error instanceof Error && "code" in error && error.code === "EEXIST")) throw error;
	}
	return validateId(await fs.readFile(file, "utf8"));
}

async function loadInstallationId(): Promise<string> {
	if (process.platform === "win32") return windowsInstallationId();
	const directory =
		process.platform === "darwin"
			? path.join(os.homedir(), "Library", "Application Support")
			: path.join(os.homedir(), ".local", "share");
	return fileInstallationId(
		path.join(directory, "LowRezStudio", "TempestCrashReports", "installation-id"),
	);
}

let installationId: Promise<string> | undefined;

export function crashInstallationId(): Promise<string> {
	installationId ??= loadInstallationId().catch((error: unknown) => {
		installationId = undefined;
		throw error;
	});
	return installationId;
}
