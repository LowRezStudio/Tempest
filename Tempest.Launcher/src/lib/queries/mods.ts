import { createMutation, createQuery, useQueryClient } from "@tanstack/svelte-query";
import { disableMod, enableMod, installMod, listMods, removeMod, renameMod } from "$lib/core/mods";

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
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

export const createRemoveModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({ gamePath, modName }: { gamePath: string; modName: string }) =>
			removeMod(gamePath, modName),
		onSuccess: (_, variables) => {
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

export const createEnableModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({ gamePath, modName }: { gamePath: string; modName: string }) =>
			enableMod(gamePath, modName),
		onSuccess: (_, variables) => {
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

export const createDisableModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({ gamePath, modName }: { gamePath: string; modName: string }) =>
			disableMod(gamePath, modName),
		onSuccess: (_, variables) => {
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

export const createRenameModMutation = () => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationFn: ({
			gamePath,
			oldName,
			newName,
		}: {
			gamePath: string;
			oldName: string;
			newName: string;
		}) => renameMod(gamePath, oldName, newName),
		onSuccess: (_, variables) => {
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};
