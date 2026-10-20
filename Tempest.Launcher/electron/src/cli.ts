// Makes the bundled CLI usable as `tempest <subcommand>` from a terminal.
// `cli:ensure` runs silently on app start (best-effort, never needs admin);
// `cli:install` is the explicit path from Settings, which may prompt for
// administrator permission on macOS to write into /usr/local/bin.
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { handle } from "./ipc/handle.js";
import type { CliInstallResult } from "./ipc/contract.js";

const SHIM_NAME = process.platform === "win32" ? "tempest.exe" : "tempest";
const CLI_NAME = process.platform === "win32" ? "tempest-cli.exe" : "tempest-cli";

/**
 * The bundled CLI sidecar binary, or null when it doesn't exist (dev runs).
 * @returns Absolute path to `tempest-cli(.exe)`, or null.
 */
function cliBinary(): string | null {
	const bin = path.join(process.resourcesPath, "binaries", CLI_NAME);
	return fs.existsSync(bin) ? bin : null;
}

async function isOnPath(target: string): Promise<boolean> {
	const dir = path.dirname(target);
	const name = path.basename(target);
	const ext = process.platform === "win32" ? ".exe" : "";
	for (const p of process.env.PATH?.split(path.delimiter) ?? []) {
		if (p !== dir) continue;
		try {
			await fs.promises.access(path.join(p, name + ext));
			return true;
		} catch {
			/* keep looking */
		}
	}
	return false;
}

function exec(command: string, args: string[], timeout = 10_000): Promise<void> {
	return new Promise((resolve, reject) => {
		execFile(command, args, { timeout, windowsHide: true }, (err) =>
			err ? reject(err) : resolve(),
		);
	});
}

/**
 * Result for dev runs, where no CLI is bundled next to Electron.
 * @returns The "cli-not-bundled" result.
 */
function notBundled(): CliInstallResult {
	return { installed: false, path: null, onPath: false, error: "cli-not-bundled" };
}

// ---- Windows ----

/**
 * Adds the binaries dir to the user PATH (idempotent) and broadcasts the change.
 * @param dir Directory containing the CLI shim.
 */
async function addToUserPath(dir: string): Promise<void> {
	const script = [
		`$b = '${dir.replaceAll("'", "''")}'`,
		"$p = [Environment]::GetEnvironmentVariable('Path','User')",
		"if (-not $p) { $p = '' }",
		"$parts = $p.Split(';') | Where-Object { $_ -and $_.Trim() }",
		"if ($parts -notcontains $b) {",
		"  $parts += $b",
		"  [Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')",
		"}",
		// Tell running processes about the change; new shells pick it up.
		"$sig = '[DllImport(\"user32.dll\", SetLastError=true, CharSet=CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'",
		"$t = Add-Type -MemberDefinition $sig -Name Win32SendMessage -Namespace Win32 -PassThru",
		"[UIntPtr]$r = [UIntPtr]::Zero",
		"$null = $t::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$r)",
	].join("\n");

	// Write to a temp file: a -Command one-liner with this much quoting is
	// unmanageable; a script file keeps it verifiable.
	const scriptPath = path.join(os.tmpdir(), `tempest-cli-path-${process.pid}.ps1`);
	await fs.promises.writeFile(scriptPath, script, "utf8");
	try {
		await exec("powershell.exe", [
			"-NoProfile",
			"-ExecutionPolicy",
			"Bypass",
			"-File",
			scriptPath,
		]);
	} finally {
		await fs.promises.rm(scriptPath, { force: true }).catch(() => {});
	}
}

async function installWindows(): Promise<CliInstallResult> {
	const bin = cliBinary();
	if (!bin) return Promise.resolve(notBundled());
	const dir = path.dirname(bin);
	const shim = path.join(dir, SHIM_NAME);
	// Hardlink (same volume, no extra disk space, runs from CreateProcess);
	// fails harmlessly if it already exists.
	if (shim !== bin) {
		await fs.promises.link(bin, shim).catch(() => {});
	}
	await addToUserPath(dir);
	return { installed: true, path: shim, onPath: true };
}

// ---- macOS ----

async function installMac(shimDir: string, elevated: boolean): Promise<CliInstallResult> {
	const bin = cliBinary();
	if (!bin) return Promise.resolve(notBundled());
	const shim = path.join(shimDir, SHIM_NAME);

	try {
		await fs.promises.mkdir(shimDir, { recursive: true });
		await fs.promises.symlink(bin, shim);
	} catch {
		if (!elevated) {
			// Silent startup pass: a read-only shim dir is not an error.
			return { installed: false, path: shim, onPath: false };
		}
		// Explicit install: ask the user for administrator permission.
		try {
			await exec(
				"osascript",
				[
					"-e",
					`do shell script "mkdir -p '${shimDir}' && ln -sf '${bin}' '${shim}'" with administrator privileges`,
				],
				60_000,
			);
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			// Error -128 is the user cancelling the prompt; not an error to surface.
			const cancelled = message.includes("-128") || message.includes("User canceled");
			return {
				installed: false,
				path: shim,
				onPath: false,
				error: cancelled ? "cancelled" : message,
			};
		}
	}
	return { installed: true, path: shim, onPath: await isOnPath(shim) };
}

// ---- Linux ----

function installLinux(): Promise<CliInstallResult> {
	const bin = cliBinary();
	if (!bin) return Promise.resolve(notBundled());

	// Prefer a system-wide link when running as root (e.g. a root-launched
	// launcher); otherwise fall back to ~/.local/bin, which needs no
	// privileges and is on PATH in most distributions.
	if (process.getuid?.() === 0) {
		return installMac("/usr/local/bin", false);
	}
	const shimDir = path.join(os.homedir(), ".local", "bin");
	return installMac(shimDir, false);
}

handle("cli:status", async (): Promise<CliInstallResult> => {
	const bin = cliBinary();
	if (!bin) return Promise.resolve(notBundled());
	const shimDir =
		process.platform === "win32"
			? path.dirname(bin)
			: process.platform === "darwin"
				? "/usr/local/bin"
				: path.join(os.homedir(), ".local", "bin");
	const shim = path.join(shimDir, SHIM_NAME);
	const exists = await fs.promises.access(shim).then(
		() => true,
		() => false,
	);
	return {
		installed: exists,
		path: exists ? shim : null,
		onPath: exists && (await isOnPath(shim)),
	};
});

handle("cli:install", (): Promise<CliInstallResult> => {
	if (process.platform === "win32") return installWindows();
	if (process.platform === "darwin") return installMac("/usr/local/bin", true);
	return installLinux();
});

/** Silent, best-effort install on app start. Never prompts and never throws. */
export function ensureCli(): void {
	const run =
		process.platform === "win32"
			? installWindows()
			: process.platform === "darwin"
				? installMac("/usr/local/bin", false)
				: installLinux();
	run.catch(() => {});
}
