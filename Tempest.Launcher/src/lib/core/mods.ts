import { join, tempDir, resolveResource } from "@tauri-apps/api/path";
import { writeFile, mkdir } from "@tauri-apps/plugin-fs";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import { getQueryClient } from "$lib/queries/client";
import { appendProcessLogs } from "$lib/stores/processes.svelte";
import { createCommand } from "./command";
import { assertIndependentPath } from "./instance-storage.svelte";
import type { Instance } from "$lib/types/instance";

const REMOTE_CORE_URL =
	"https://github.com/LowRezStudio/TgMod/releases/download/1.0.0/Tempest.Core.Tempest";

async function downloadRemoteMod(url: string, filename: string): Promise<string> {
	const tmp = await tempDir();
	const dest = await join(tmp, filename);
	try {
		const res = await tauriFetch(url, { method: "GET" });
		if (!res.ok) throw new Error(`HTTP ${res.status}`);
		const buf = new Uint8Array(await res.arrayBuffer());
		await mkdir(tmp, { recursive: true }).catch(() => {});
		await writeFile(dest, buf);
		return dest;
	} catch (error) {
		console.error(`Failed to download ${url}:`, error);
		throw error;
	}
}

export type ModAuthor = {
	Name: string;
	Link: string;
	Avatar?: string;
};

export type ModRecord = {
	Id: string;
	Name: string;
	Author: string;
	Authors?: ModAuthor[];
	Version: string;
	Enabled: boolean;
	Kind: string;
	OriginalPath: string;
	InstalledFiles: string[];
	OwnedFiles: string[];
	Readme?: string;
	ReadmeContent?: string;
};

export type ModConflictInfo = {
	ModId: string;
	ModName: string;
	ModVersion?: string;
	ConflictingFiles: string[];
};

export type ModInstallResult = {
	Success: boolean;
	Message: string;
	Mod?: ModRecord;
	Conflict?: boolean;
	IsModConflict?: boolean;
	Unverified?: boolean;
	ConflictingMods?: ModConflictInfo[];
	NewModName?: string;
};

export type ModListResult = {
	Mods: ModRecord[];
};

/** Versions that get Tempest Core instead of Tempest Console. */
const CORE_MOD_VERSIONS = new Set(["0.56", "0.57"]);

/** Versions that should skip auto-mod installation entirely. */
const SKIP_AUTO_MOD_VERSIONS = new Set(["8.1"]);

export const installAutoMods = async (instance: Instance): Promise<void> => {
	const gamePath = instance?.path;
	if (!gamePath) return;

	if (instance.version && SKIP_AUTO_MOD_VERSIONS.has(instance.version)) return;

	const isCoreVersion = !!(instance.version && CORE_MOD_VERSIONS.has(instance.version));

	if (isCoreVersion) {
		// Fetch latest Tempest Core from GitHub so you can update it without rebuilding the launcher
		try {
			appendProcessLogs([`Fetching Tempest Core from ${REMOTE_CORE_URL}`], false, "mods");
			const modFile = await downloadRemoteMod(REMOTE_CORE_URL, "Tempest.Core.Tempest");
			await installMod(gamePath, modFile, true, true);
		} catch (error) {
			console.error("Failed to fetch remote Tempest Core, falling back to bundled:", error);
			try {
				const fallback = await resolveResource("Tempest Core.tempest");
				await installMod(gamePath, fallback, true, true);
			} catch (error) {
				console.error("Failed to install fallback Tempest Core:", error);
			}
		}
	} else {
		try {
			const modFile = await resolveResource("Tempest Console.tempest");
			await installMod(gamePath, modFile, true, true);
		} catch (error) {
			console.error("Failed to install Tempest Console:", error);
		}
	}

	void getQueryClient()?.invalidateQueries({ queryKey: ["mods", gamePath] });
};

/**
 * @deprecated Core installs now belong to explicit per-instance setup. Kept for
 * existing startup callers, which must never replace mods across the library.
 * @param _instances Legacy library argument.
 * @returns Completion without changing game files.
 */
export const checkForCoreUpdatesAndInstall = (_instances: Instance[]): Promise<void> =>
	Promise.resolve();

export const listMods = async (gamePath: string): Promise<ModRecord[]> => {
	const args = ["mod", "list", gamePath, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	if (res.code !== 0) return [];
	try {
		const parsed = JSON.parse(res.stdout) as ModListResult;
		return parsed.Mods || [];
	} catch (error) {
		console.error("Failed to parse mods list:", error);
		return [];
	}
};

export const installMod = async (
	gamePath: string,
	modFile: string,
	replace = false,
	allowUnsigned = false,
	stack = false,
): Promise<ModInstallResult> => {
	await assertIndependentPath(gamePath);
	const args = ["mod", "install", gamePath, modFile];
	if (replace) args.push("--replace");
	if (stack) args.push("--stack");
	if (allowUnsigned) args.push("--allow-unsigned");
	args.push("--json");

	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse install mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const removeMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	await assertIndependentPath(gamePath);
	const args = ["mod", "remove", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse remove mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const renameMod = async (
	gamePath: string,
	oldName: string,
	newName: string,
): Promise<ModInstallResult> => {
	await assertIndependentPath(gamePath);
	const args = ["mod", "rename", gamePath, oldName, newName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse rename mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const enableMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	await assertIndependentPath(gamePath);
	const args = ["mod", "enable", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse enable mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const disableMod = async (gamePath: string, modName: string): Promise<ModInstallResult> => {
	await assertIndependentPath(gamePath);
	const args = ["mod", "disable", gamePath, modName, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse disable mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};

export const reloadModDll = async (gamePath: string, modId: string): Promise<ModInstallResult> => {
	const args = ["mod", "reload-dll", gamePath, modId, "--json"];
	appendProcessLogs([`Running command: tempest-cli ${args.join(" ")}`], false, "mods");
	const res = await createCommand(args).execute();

	if (res.stdout) {
		appendProcessLogs(res.stdout.split("\n").filter(Boolean), false, "mods");
	}
	if (res.stderr) {
		appendProcessLogs(res.stderr.split("\n").filter(Boolean), true, "mods");
	}

	try {
		return JSON.parse(res.stdout) as ModInstallResult;
	} catch (error) {
		console.error("Failed to parse reload-dll mod result:", error, res.stdout, res.stderr);
		return { Success: false, Message: "Internal error parsing CLI output" };
	}
};
