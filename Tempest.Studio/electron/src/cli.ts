// tempest-cli for the mod cooker: packing, signing, verifying, installing
// and publishing `.tempest` packages, exactly as the launcher does. Found at
// $TEMPEST_CLI, else bundled next to `upk` (packaged builds), else the
// repository's Debug/Release build (dev). Only `mod` subcommands run.
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";

const EXE = process.platform === "win32" ? "tempest-cli.exe" : "tempest-cli";
const DEV_EXE = process.platform === "win32" ? "Tempest.CLI.exe" : "Tempest.CLI";
const ALLOWED = new Set([
	"pack",
	"sign",
	"verify",
	"install",
	"publish",
	"login",
	"gen-key",
	"fingerprint",
	"search",
]);

function cliPath(): string | null {
	const env = process.env.TEMPEST_CLI;
	if (env && fs.existsSync(env)) return env;
	const bundled = path.join(process.resourcesPath, "binaries", EXE);
	if (app.isPackaged && fs.existsSync(bundled)) return bundled;
	const repo = path.resolve(import.meta.dirname, "..", "..", "..", "Tempest.CLI", "bin");
	for (const config of ["Release", "Debug"]) {
		const p = path.join(repo, config, "net10.0", DEV_EXE);
		if (fs.existsSync(p)) return p;
	}
	return null;
}

/** The key `tempest mod sign` uses: $TEMPEST_SIGNING_KEY, else the app-data default. */
function signingKey(): string {
	return (
		process.env.TEMPEST_SIGNING_KEY ??
		path.join(app.getPath("appData"), "com.lowrezstudio.tempest", "signing", "tempest.key")
	);
}

handle("cli:status", () => {
	const cli = cliPath();
	const key = signingKey();
	return { path: cli, available: cli !== null, signingKey: key, hasKey: fs.existsSync(key) };
});

handle("cli:mod", (_event, { args }) => {
	const cli = cliPath();
	if (!cli) {
		return Promise.reject(
			new Error("tempest-cli was not found (build Tempest.CLI, or set TEMPEST_CLI)"),
		);
	}
	if (!ALLOWED.has(args[0] ?? "")) {
		return Promise.reject(new Error(`mod ${args[0]} is not available here`));
	}
	return new Promise((resolve, reject) => {
		const child = spawn(cli, ["mod", ...args], { windowsHide: true });
		let stdout = "";
		let stderr = "";
		child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
		child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
		const timer = setTimeout(() => child.kill(), 10 * 60 * 1000);
		child.on("error", (error) => {
			clearTimeout(timer);
			reject(error);
		});
		child.on("close", (code) => {
			clearTimeout(timer);
			resolve({ code: code ?? -1, stdout, stderr });
		});
	});
});

/** A fresh staging folder for a cooked mod. */
handle("cli:staging", (_event, { id }) => {
	const dir = path.join(app.getPath("temp"), "tempest-studio", "cook", `${id}-${Date.now()}`);
	fs.mkdirSync(path.dirname(dir), { recursive: true });
	return dir;
});
