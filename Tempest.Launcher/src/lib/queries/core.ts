import { createMutation } from "@tanstack/svelte-query";
import type { Instance } from "$lib/types/instance";
import { identifyBuild, killGame, launchGame, type BuildInfo } from "$lib/core/index";

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
