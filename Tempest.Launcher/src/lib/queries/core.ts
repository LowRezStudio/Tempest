import { createMutation } from "@tanstack/svelte-query";
import { identifyBuild, killGame, launchGame } from "$lib/core/index";
import type { BuildInfo } from "$lib/core/index";
import type { Instance } from "$lib/types/instance";

export const createLaunchGameMutation = () =>
	createMutation(() => ({
		mutationFn: (instance: Instance) => launchGame(instance),
	}));

export const createKillGameMutation = () =>
	createMutation(() => ({
		mutationFn: (instance: Instance) => killGame(instance),
	}));

/**
 * Identifying a game folder's build, cached for the session: it hashes the game's largest files.
 * @param path - The game folder.
 * @returns Query options for createQuery/createQueries/fetchQuery.
 */
export const buildQueryOptions = (path: string) => ({
	queryKey: ["build", path],
	queryFn: (): Promise<BuildInfo | null> => identifyBuild(path),
	staleTime: Infinity,
});
