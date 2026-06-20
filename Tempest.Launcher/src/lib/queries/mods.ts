import { createMutation, createQuery, useQueryClient } from "@tanstack/svelte-query";
import { installMod, listMods, removeMod } from "$lib/core/mods";

export const createModsQuery = (instancePath: () => string) =>
	createQuery(() => {
		const pathValue = instancePath();
		return {
			queryKey: ["mods", pathValue],
			queryFn: () => listMods(pathValue),
			enabled: !!pathValue,
		};
	});

export const createInstallModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({
			gamePath,
			modFile,
			replace,
		}: {
			gamePath: string;
			modFile: string;
			replace?: boolean;
		}) => installMod(gamePath, modFile, replace),
		onSuccess: (_, variables) => {
			queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

export const createRemoveModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({ gamePath, modName }: { gamePath: string; modName: string }) =>
			removeMod(gamePath, modName),
		onSuccess: (_, variables) => {
			queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};
