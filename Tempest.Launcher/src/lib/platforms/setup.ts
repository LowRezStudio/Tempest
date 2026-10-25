import { createCommand, processArgs, type ArgumentType } from "$lib/core/command";
import { getInstancePaths } from "$lib/core/paths";
import { invoke } from "$lib/platform/api";
import { updateInstance } from "$lib/stores/instance.svelte";
import { appendProcessLog, appendProcessLogs } from "$lib/stores/processes.svelte";
import type { GameLayout } from "$ipc/contract";
import type { Instance } from "$lib/types/instance";

const SOURCE = "setup";

const log = (line: string, error = false): void => appendProcessLog(line, error, SOURCE);

const logCommand = (args: ArgumentType[]): void =>
	log(`Running command: ${processArgs(args).join(" ")}`);

/**
 * Ensures non-8.1 instances launch into the shooting range, like a user-added arg.
 * @param instance Instance to update in place.
 */
const ensureShootingRangeArg = (instance: Instance): void => {
	if (instance.version === "8.1") return;
	const args = instance.launchOptions?.args ?? [];
	if (args.some((a) => a.startsWith("Shootingrange_P"))) return;
	const updated = {
		...instance,
		launchOptions: {
			...instance.launchOptions,
			args: ["Shootingrange_P?game=ShootingRange", ...args],
		},
	};
	updateInstance(instance.id, updated);
};

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

	ensureShootingRangeArg(instance);

	// Resolve the game layout (game folder, token sources, assembly.dat) in one
	// host-side operation; previously this was a filesystem walk over IPC.
	const layout = await invoke("instance:game-layout", { instancePath: instance.path });
	log(`Game folder found: ${layout.gameFolder}`);

	const paths = await getInstancePaths(instance.id);
	log("Instance paths resolved");

	const platform = instance.launchOptions.platform ?? "Win64";
	const preferredSucceeded = await tryExtractTokenSources(
		layout.tokenSources[platform],
		paths.tokensDir,
	);
	if (preferredSucceeded) {
		await exportAssembly(layout, paths.assemblyDb, paths.fieldsToken, paths.functionsToken);
		log("Instance setup complete");
		return;
	}

	if (platform !== "Win32") {
		const win32Succeeded = await tryExtractTokenSources(
			layout.tokenSources.Win32,
			paths.tokensDir,
		);
		if (win32Succeeded) {
			await exportAssembly(layout, paths.assemblyDb, paths.fieldsToken, paths.functionsToken);
			log("Instance setup complete");
			return;
		}
	}

	const message = "Failed to extract tokens from Paladins.exe or MctsInterface.dll.";
	log(message, true);
	throw new Error(message);
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

const exportAssembly = async (
	layout: GameLayout,
	assemblyDbPath: string,
	fieldsPath: string,
	functionsPath: string,
): Promise<void> => {
	const assemblyDatPath = layout.assemblyDat;

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
