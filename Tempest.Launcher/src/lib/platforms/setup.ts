import { path as tauriPath } from "@tauri-apps/api";
import { readDir } from "@tauri-apps/plugin-fs";
import { createCommand } from "$lib/core/command";
import { getInstanceAssemblyDbPath, getInstanceTokensDir } from "$lib/core/paths";
import { allowScopeDirectory } from "$lib/tauri/scopes";
import type { Instance, InstancePlatform } from "$lib/types/instance";

const defaultGameExe = "Paladins.exe";
const fallbackTokenDll = "MctsInterface.dll";

export const setupInstance = async (instance: Instance): Promise<void> => {
	await allowScopeDirectory(instance.path, true);

	const tokensDir = await getInstanceTokensDir(instance.id);
	await allowScopeDirectory(tokensDir, true);

	const platform = instance.launchOptions.platform ?? "Win64";
	const preferredSources = await resolveTokenSources(instance.path, platform);
	const preferredSucceeded = await tryExtractTokenSources(preferredSources, tokensDir);
	if (preferredSucceeded) {
		await exportAssembly(instance.path, instance.id);
		return;
	}

	if (platform !== "Win32") {
		const win32Sources = await resolveTokenSources(instance.path, "Win32");
		const win32Succeeded = await tryExtractTokenSources(win32Sources, tokensDir);
		if (win32Succeeded) {
			await exportAssembly(instance.path, instance.id);
			return;
		}
	}

	throw new Error("Failed to extract tokens from Paladins.exe or MctsInterface.dll.");
};

const resolveTokenSources = async (
	instancePath: string,
	platform: InstancePlatform,
): Promise<{ primary: string; fallback: string }> => {
	const startPath =
		isFilePath(instancePath) ? await tauriPath.dirname(instancePath) : instancePath;
	const gameFolder = await findGameFolder(startPath);
	await allowScopeDirectory(gameFolder, true);

	const primaryPath = await tauriPath.join(gameFolder, "Binaries", platform, defaultGameExe);
	const fallbackPath = await tauriPath.join(gameFolder, "Binaries", platform, fallbackTokenDll);

	return {
		primary: primaryPath,
		fallback: fallbackPath,
	};
};

const resolveAssemblyDatPath = async (instancePath: string): Promise<string> => {
	const startPath =
		isFilePath(instancePath) ? await tauriPath.dirname(instancePath) : instancePath;
	const gameFolder = await findGameFolder(startPath);
	const assemblyDir = await tauriPath.join(gameFolder, "ChaosGame", "CookedPCConsole");
	await allowScopeDirectory(assemblyDir, true);
	return tauriPath.join(assemblyDir, "assembly.dat");
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

const exportAssembly = async (instancePath: string, instanceId: string): Promise<void> => {
	const tokensDir = await getInstanceTokensDir(instanceId);
	const fieldsPath = await tauriPath.join(tokensDir, "fields.dat");
	const functionsPath = await tauriPath.join(tokensDir, "functions.dat");
	const assemblyDbPath = await getInstanceAssemblyDbPath(instanceId);
	const assemblyDatPath = await resolveAssemblyDatPath(instancePath);

	const baseArgs = {
		"--format": "Sqlite",
		"--fields": fieldsPath,
		"--functions": functionsPath,
		"--path": assemblyDatPath,
		"--obscure": true,
		"--output": assemblyDbPath,
	};

	const legacyResult = await createCommand([
		"marshal",
		"deserialize",
		{ ...baseArgs, "--version": "Legacy" },
	]).execute();
	if (legacyResult.code === 0) return;

	const modernResult = await createCommand([
		"marshal",
		"deserialize",
		{ ...baseArgs, "--version": "Modern" },
	]).execute();
	if (modernResult.code !== 0) {
		const legacyStdout = legacyResult.stdout?.trim();
		const legacyStderr = legacyResult.stderr?.trim();
		const modernStdout = modernResult.stdout?.trim();
		const modernStderr = modernResult.stderr?.trim();
		throw new Error(
			[
				"Failed to deserialize assembly tokens.",
				legacyStderr && `Legacy stderr: ${legacyStderr}`,
				legacyStdout && `Legacy stdout: ${legacyStdout}`,
				modernStderr && `Modern stderr: ${modernStderr}`,
				modernStdout && `Modern stdout: ${modernStdout}`,
			]
				.filter(Boolean)
				.join(" "),
		);
	}
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
