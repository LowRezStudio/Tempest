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

export const createIdentifyBuildMutation = () =>
	createMutation(() => ({
		mutationFn: (path: string): Promise<BuildInfo | null> => identifyBuild(path),
	}));
