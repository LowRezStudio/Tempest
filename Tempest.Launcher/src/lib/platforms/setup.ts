import { path as tauriPath } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { readDir } from "@tauri-apps/plugin-fs";
import { createCommand } from "$lib/core/command";
import { allowScopeDirectory } from "$lib/tauri/scopes";
import type { Instance, InstancePlatform } from "$lib/types/instance";

const defaultGameExe = "Paladins.exe";
const fallbackTokenDll = "MctsInterface.dll";

export const setupInstance = async (instance: Instance): Promise<void> => {
	await allowScopeDirectory(instance.path, true);

	const tokensDir = await getTokensDir(instance.id);
	await allowScopeDirectory(tokensDir, true);

	const platform = instance.launchOptions.platform ?? "Win64";
	const preferredSources = await resolveTokenSources(instance.path, platform);
	const preferredSucceeded = await tryExtractTokenSources(preferredSources, tokensDir);
	if (preferredSucceeded) return;

	if (platform !== "Win32") {
		const win32Sources = await resolveTokenSources(instance.path, "Win32");
		const win32Succeeded = await tryExtractTokenSources(win32Sources, tokensDir);
		if (win32Succeeded) return;
	}

	throw new Error("Failed to extract tokens from Paladins.exe or MctsInterface.dll.");
};

export const getInstanceTempestPath = async (id: string): Promise<string> => {
	const home = await homeDir();
	return tauriPath.join(home, ".tempest", "instances", id);
};

export const getFieldsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getTokensDir(id);
	return tauriPath.join(tokensDir, "fields.dat");
};

export const getFunctionsTokenPath = async (id: string): Promise<string> => {
	const tokensDir = await getTokensDir(id);
	return tauriPath.join(tokensDir, "functions.dat");
};

const getTokensDir = async (id: string): Promise<string> => {
	const baseDir = await getInstanceTempestPath(id);
	return tauriPath.join(baseDir, "tokens");
};

const resolveTokenSources = async (
	instancePath: string,
	platform: InstancePlatform,
): Promise<{ primary: string; fallback: string }> => {
	const startPath = isFilePath(instancePath) ? await tauriPath.dirname(instancePath) : instancePath;
	const gameFolder = await findGameFolder(startPath);
	await allowScopeDirectory(gameFolder, true);

	const primaryPath = await tauriPath.join(
		gameFolder,
		"Binaries",
		platform,
		defaultGameExe,
	);
	const fallbackPath = await tauriPath.join(
		gameFolder,
		"Binaries",
		platform,
		fallbackTokenDll,
	);

	return {
		primary: primaryPath,
		fallback: fallbackPath,
	};
};

const tryExtractTokenSources = async (
	sources: { primary: string; fallback: string },
	outputDir: string,
): Promise<boolean> => {
	const primaryResult = await tryExtractTokens(sources.primary, outputDir);
	if (primaryResult) return true;
	return await tryExtractTokens(sources.fallback, outputDir);
};

const tryExtractTokens = async (path: string, outputDir: string): Promise<boolean> => {
	const result = await createCommand([
		"marshal",
		"extract-tokens",
		{ "--path": path, "--output": outputDir },
	]).execute();

	return result.code === 0;
};

const isFilePath = (value: string): boolean => {
	const lower = value.toLowerCase();
	return lower.endsWith(".exe") || lower.endsWith(".dll");
};

const findGameFolder = async (startPath: string): Promise<string> => {
	let current = startPath;

	while (true) {
		if (await hasRequiredGameDirs(current)) return current;

		const parent = await tauriPath.dirname(current);
		if (parent === current) break;
		current = parent;
	}

	throw new Error(
		"Couldn't find the Paladins game folder (containing Binaries and Engine folders)",
	);
};

const hasRequiredGameDirs = async (dirPath: string): Promise<boolean> => {
	try {
		const entries = await readDir(dirPath);
		let hasBinaries = false;
		let hasEngine = false;
		for (const entry of entries) {
			if (!entry.isDirectory) continue;
			if (entry.name === "Binaries") hasBinaries = true;
			if (entry.name === "Engine") hasEngine = true;
			if (hasBinaries && hasEngine) return true;
		}
		return false;
	} catch {
		return false;
	}
};
