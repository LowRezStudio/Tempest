// Mod publishing setup status: what the launcher knows about the signing key
// and the repository API key the CLI stores. Reads the same files the CLI
// writes (`signing/tempest.{key,pub}`, `repo.json`) under the app-data dir
// .NET resolves (on macOS that is ~/.config, not Electron's appData).
import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";

const CLI_NAME = process.platform === "win32" ? "tempest-cli.exe" : "tempest-cli";

type CliRun = { program: string; prefix: string[] };

/**
 * The bundled CLI, or the dev debug build via `dotnet exec`; null when neither exists.
 * @returns Program and argument prefix for exec'ing the CLI.
 */
function resolveCli(): CliRun | null {
	const bundled = path.join(process.resourcesPath, "binaries", CLI_NAME);
	if (fs.existsSync(bundled)) return { program: bundled, prefix: [] };
	// Dev run: getAppPath() is Tempest.Launcher/electron; the repo root is two up.
	const dll = path.resolve(
		app.getAppPath(),
		"..",
		"..",
		"Tempest.CLI",
		"bin",
		"Debug",
		"net10.0",
		"Tempest.CLI.dll",
	);
	return fs.existsSync(dll) ? { program: "dotnet", prefix: ["exec", dll] } : null;
}

function run(
	cli: CliRun,
	args: string[],
	timeout = 30_000,
): Promise<{ code: number; stdout: string; stderr: string }> {
	return new Promise((resolve, reject) => {
		execFile(
			cli.program,
			[...cli.prefix, ...args],
			{ timeout, windowsHide: true },
			(err, stdout, stderr) => {
				if (err && typeof err.code !== "number") return reject(err);
				resolve({
					code: err ? (typeof err.code === "number" ? err.code : 1) : 0,
					stdout: stdout?.toString() ?? "",
					stderr: stderr?.toString() ?? "",
				});
			},
		);
	});
}

/**
 * The data dir the CLI uses. .NET's SpecialFolder.ApplicationData maps to
 * Electron's appData on every platform (%APPDATA% on Windows, ~/Library/
 * Application Support on macOS, ~/.config on Linux).
 * @returns Absolute path of the `com.lowrezstudio.tempest` data directory.
 */
function tempestDataDir(): string {
	return app.getPath("appData");
}

const DATA_DIR = path.join(tempestDataDir(), "com.lowrezstudio.tempest");
const PUB_KEY_PATH = path.join(DATA_DIR, "signing", "tempest.pub");
const REPO_CONFIG_PATH = path.join(DATA_DIR, "repo.json");

type RepoConfigJson = { url?: string; apiKey?: string };

function readRepoConfig(): RepoConfigJson {
	try {
		return JSON.parse(fs.readFileSync(REPO_CONFIG_PATH, "utf8")) as RepoConfigJson;
	} catch {
		return {};
	}
}

function maskApiKey(key: string): string {
	if (key.length <= 12) return `${key.slice(0, 4)}…`;
	return `${key.slice(0, 8)}…${key.slice(-4)}`;
}

handle("signing:status", async () => {
	const hasKey = fs.existsSync(PUB_KEY_PATH);
	if (!hasKey) return { hasKey: false, keyId: null };
	const cli = resolveCli();
	if (!cli) return { hasKey: true, keyId: null };
	try {
		// Prints "<user id> [<fingerprint>]".
		const res = await run(cli, ["mod", "fingerprint", PUB_KEY_PATH]);
		const id = res.stdout.trim();
		return { hasKey: true, keyId: id !== "" ? id : null };
	} catch {
		return { hasKey: true, keyId: null };
	}
});

handle("repo:status", () => {
	const config = readRepoConfig();
	const apiKey = config.apiKey;
	return {
		hasApiKey: !!apiKey,
		masked: apiKey ? maskApiKey(apiKey) : null,
	};
});

handle("repo:forget", async () => {
	// `tempest mod login` with no arguments forgets the stored key.
	const cli = resolveCli();
	if (!cli) return;
	await run(cli, ["mod", "login"]).catch(() => {});
});
