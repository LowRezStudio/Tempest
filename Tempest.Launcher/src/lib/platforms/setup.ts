import { path as tauriPath } from "@tauri-apps/api";
import { readDir } from "@tauri-apps/plugin-fs";
import { createCommand, processArgs, type ArgumentType } from "$lib/core/command";
import { installAutoMods } from "$lib/core/mods";
import { getInstanceAssemblyDbPath, getInstanceTokensDir } from "$lib/core/paths";
import { appendProcessLog, appendProcessLogs } from "$lib/stores/processes.svelte";
import { allowScopeDirectory } from "$lib/tauri/scopes";
import type { Instance, InstancePlatform } from "$lib/types/instance";

const defaultGameExe = "Paladins.exe";
const fallbackTokenDll = "MctsInterface.dll";

const SOURCE = "setup";

const log = (line: string, error = false): void => appendProcessLog(line, error, SOURCE);

const logCommand = (args: ArgumentType[]): void =>
	log(`Running command: ${processArgs(args).join(" ")}`);

const logResult = (result: { stdout?: string; stderr?: string }): void => {
	if (result.stdout) {
		appendProcessLogs(result.stdout.split("\n").filter(Boolean), false, SOURCE);
	}
	if (result.stderr) {
		appendProcessLogs(result.stderr.split("\n").filter(Boolean), true, SOURCE);
	}
};

export const setupInstance = async (instance: Instance): Promise<void> => {
	log(`Setting up instance "${instance.label}" (${instance.path})`);

	log("Installing auto mods...");
	await installAutoMods(instance);
	log("Auto mods installed");

	await allowScopeDirectory(instance.path, true);

	const tokensDir = await getInstanceTokensDir(instance.id);
	await allowScopeDirectory(tokensDir, true);
	log("File system scopes granted");

	const platform = instance.launchOptions.platform ?? "Win64";
	const preferredSources = await resolveTokenSources(instance.path, platform);
	const preferredSucceeded = await tryExtractTokenSources(preferredSources, tokensDir);
	if (preferredSucceeded) {
		await exportAssembly(instance.path, instance.id);
		log("Instance setup complete");
		return;
	}

	if (platform !== "Win32") {
		const win32Sources = await resolveTokenSources(instance.path, "Win32");
		const win32Succeeded = await tryExtractTokenSources(win32Sources, tokensDir);
		if (win32Succeeded) {
			await exportAssembly(instance.path, instance.id);
			log("Instance setup complete");
			return;
		}
	}

	const message = "Failed to extract tokens from Paladins.exe or MctsInterface.dll.";
	log(message, true);
	throw new Error(message);
};

const resolveTokenSources = async (
	instancePath: string,
	platform: InstancePlatform,
): Promise<{ primary: string; fallback: string }> => {
	const startPath = isFilePath(instancePath)
		? await tauriPath.dirname(instancePath)
		: instancePath;
	const gameFolder = await findGameFolder(startPath);
	log(`Game folder found: ${gameFolder}`);
	await allowScopeDirectory(gameFolder, true);

	const primaryPath = await tauriPath.join(gameFolder, "Binaries", platform, defaultGameExe);
	const fallbackPath = await tauriPath.join(gameFolder, "Binaries", platform, fallbackTokenDll);

	return {
		primary: primaryPath,
		fallback: fallbackPath,
	};
};

const resolveAssemblyDatPath = async (instancePath: string): Promise<string> => {
	const startPath = isFilePath(instancePath)
		? await tauriPath.dirname(instancePath)
		: instancePath;
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
	log(`Extracting tokens from ${path}`);
	const args: ArgumentType[] = [
		"marshal",
		"extract-tokens",
		{ "--path": path, "--output": outputDir },
	];
	logCommand(args);
	const result = await createCommand(args).execute();
	logResult(result);

	if (result.code !== 0) {
		log(`Token extraction failed with exit code ${result.code}`, true);
		return false;
	}
	return true;
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

	log(`Exporting assembly database to ${assemblyDbPath}`);

	const legacyArgs: ArgumentType[] = [
		"marshal",
		"deserialize",
		{ ...baseArgs, "--version": "Legacy" },
	];
	logCommand(legacyArgs);
	const legacyResult = await createCommand(legacyArgs).execute();
	logResult(legacyResult);
	if (legacyResult.code === 0) return;

	const modernArgs: ArgumentType[] = [
		"marshal",
		"deserialize",
		{ ...baseArgs, "--version": "Modern" },
	];
	logCommand(modernArgs);
	const modernResult = await createCommand(modernArgs).execute();
	logResult(modernResult);
	if (modernResult.code !== 0) {
		const legacyStdout = legacyResult.stdout?.trim();
		const legacyStderr = legacyResult.stderr?.trim();
		const modernStdout = modernResult.stdout?.trim();
		const modernStderr = modernResult.stderr?.trim();
		const message = [
			"Failed to deserialize assembly tokens.",
			legacyStderr && `Legacy stderr: ${legacyStderr}`,
			legacyStdout && `Legacy stdout: ${legacyStdout}`,
			modernStderr && `Modern stderr: ${modernStderr}`,
			modernStdout && `Modern stdout: ${modernStdout}`,
		]
			.filter(Boolean)
			.join(" ");
		log(message, true);
		throw new Error(message);
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

	const message =
		"Couldn't find the Paladins game folder (containing Binaries and Engine folders)";
	log(message, true);
	throw new Error(message);
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
