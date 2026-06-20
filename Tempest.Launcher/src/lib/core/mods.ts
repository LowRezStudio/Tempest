import { appendProcessLogs } from "$lib/stores/processes";
import { createCommand } from "./command";

export type ModRecord = {
	Id: string;
	Name: string;
	Author: string;
	Version: string;
	Enabled: boolean;
	Kind: string;
	OriginalPath: string;
	InstalledFiles: string[];
};

export type ModInstallResult = {
	Success: boolean;
	Message: string;
	Mod?: ModRecord;
	Conflict?: boolean;
};

export type ModListResult = {
	Mods: ModRecord[];
};

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
): Promise<ModInstallResult> => {
	const args = ["mod", "install", gamePath, modFile];
	if (replace) args.push("--replace");
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
