// Instance data layout: path computation, deletion and game-folder discovery
// all live host-side so the renderer never touches the filesystem directly.
import fs from "node:fs";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";
import { resolveGameFolder } from "./platforms.js";
import type { GamePlatform } from "./ipc/contract.js";

const ALL_PLATFORMS: GamePlatform[] = ["Win64", "Win32"];

handle("instance:paths", (_event, { id }) => {
	const base = path.join(app.getPath("userData"), "instances", id);
	return {
		base,
		tokensDir: path.join(base, "tokens"),
		fieldsToken: path.join(base, "tokens", "fields.dat"),
		functionsToken: path.join(base, "tokens", "functions.dat"),
		assemblyDb: path.join(base, "assembly.db"),
	};
});

handle("instance:delete-data", async (_event, { instancePath }) => {
	try {
		await fs.promises.rm(instancePath, { recursive: true });
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		throw new Error(`failed to remove path ${instancePath}: ${message}`);
	}
});

handle("instance:game-layout", async (_event, { instancePath }) => {
	const gameFolder = await resolveGameFolder(instancePath);
	if (!gameFolder) {
		throw new Error(
			"Couldn't find the Paladins game folder (containing Binaries and Engine folders)",
		);
	}

	const tokenSources = Object.fromEntries(
		ALL_PLATFORMS.map((platform) => [
			platform,
			{
				primary: path.join(gameFolder, "Binaries", platform, "Paladins.exe"),
				fallback: path.join(gameFolder, "Binaries", platform, "MctsInterface.dll"),
			},
		]),
	) as Record<GamePlatform, { primary: string; fallback: string }>;

	return {
		gameFolder,
		tokenSources,
		assemblyDat: path.join(gameFolder, "ChaosGame", "CookedPCConsole", "assembly.dat"),
	};
});
