import { path as tauriPath } from "@tauri-apps/api";
import { homeDir } from "@tauri-apps/api/path";
import { createCommand } from "$lib/core";
import type { Instance } from "$lib/types/instance";

export type TokenExtractionResult = {
	success: boolean;
	stdout: string;
	stderr: string;
};

const getTempestHome = async () =>
	tauriPath.join(await homeDir(), ".tempest");

const buildExecutableCandidates = async (instancePath: string, platform: string | undefined) => {
	const extension = await tauriPath.extname(instancePath);
	if (extension?.toLowerCase() === ".exe" || extension?.toLowerCase() === ".dll") {
		return [instancePath];
	}

	const preferredPlatform = platform ?? "Win64";
	const rootCandidates = [
		instancePath,
		await tauriPath.dirname(instancePath),
		await tauriPath.dirname(await tauriPath.dirname(instancePath)),
	];

	const candidates: string[] = [];
	for (const root of rootCandidates) {
		candidates.push(await tauriPath.join(root, "Binaries", preferredPlatform, "Paladins.exe"));
		candidates.push(await tauriPath.join(root, "Binaries", "Win32", "Paladins.exe"));
	}

	return candidates;
};

export const extractInstanceTokens = async (instance: Instance): Promise<TokenExtractionResult> => {
	const tempestHome = await getTempestHome();
	const outputDir = await tauriPath.join(tempestHome, "instances", instance.id);

	const candidatePaths = await buildExecutableCandidates(
		instance.path,
		instance.launchOptions?.platform,
	);

	let lastResult: TokenExtractionResult = { success: false, stdout: "", stderr: "" };

	for (const candidate of candidatePaths) {
		const result = await createCommand([
			"marshal",
			"extract-tokens",
			{ "--path": candidate, "--output": outputDir },
		]).execute();
		if (result.code === 0) {
			return { success: true, stdout: result.stdout, stderr: result.stderr };
		}
		lastResult = { success: false, stdout: result.stdout, stderr: result.stderr };

		const candidateDir = await tauriPath.dirname(candidate);
		const fallbackPath = await tauriPath.join(candidateDir, "MctsInterface.dll");
		const fallback = await createCommand([
			"marshal",
			"extract-tokens",
			{ "--path": fallbackPath, "--output": outputDir },
		]).execute();
		if (fallback.code === 0) {
			return { success: true, stdout: fallback.stdout, stderr: fallback.stderr };
		}
		lastResult = { success: false, stdout: fallback.stdout, stderr: fallback.stderr };
	}

	return lastResult;
};
