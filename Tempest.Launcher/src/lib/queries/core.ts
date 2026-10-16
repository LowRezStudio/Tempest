import { createMutation } from "@tanstack/svelte-query";
import { identifyBuild, killGame, launchGame } from "$lib/core/index";
import { prepareIndependentInstances } from "$lib/core/instance-storage.svelte";
import { instanceMap } from "$lib/stores/instance.svelte";
import type { BuildInfo } from "$lib/core/index";
import type { Instance } from "$lib/types/instance";

export const createLaunchGameMutation = () =>
	createMutation(() => ({
		mutationFn: async (instance: Instance) => {
			await prepareIndependentInstances();
			const current = instanceMap.value[instance.id];
			if (!current) throw new Error("This instance is no longer in the library.");
			return launchGame(current);
		},
	}));

export const createKillGameMutation = () =>
	createMutation(() => ({
		mutationFn: (instance: Instance) => killGame(instance),
	}));

export const createIdentifyBuildMutation = () =>
	createMutation(() => ({
		mutationFn: (path: string): Promise<BuildInfo | null> => identifyBuild(path),
	}));
