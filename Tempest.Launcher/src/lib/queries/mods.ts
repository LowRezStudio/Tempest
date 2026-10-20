import { createMutation, createQuery, useQueryClient } from "@tanstack/svelte-query";
import { disableMod, enableMod, installMod, listMods, removeMod, renameMod } from "$lib/core/mods";
import type { ModInstallResult, ModRecord } from "$lib/core/mods";

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

// Applies the change to the cached list immediately instead of waiting for the CLI,
// rolls it back on failure, and resyncs once no other mod edit is in flight.
const createOptimisticModMutation = <V extends { gamePath: string }>(
	run: (variables: V) => Promise<ModInstallResult>,
	patch: (mod: ModRecord, variables: V) => ModRecord,
) => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationKey: ["mods-edit"],
		mutationFn: async (variables: V) => {
			const res = await run(variables);
			if (!res.Success) throw new Error(res.Message || "Unknown error");
			return res;
		},
		onMutate: async (variables: V) => {
			const queryKey = ["mods", variables.gamePath];
			await queryClient.cancelQueries({ queryKey });
			const previous = queryClient.getQueryData<ModRecord[]>(queryKey);
			queryClient.setQueryData<ModRecord[]>(queryKey, (mods) =>
				mods?.map((mod) => patch(mod, variables)),
			);
			return { previous };
		},
		onError: (_error, variables, result) => {
			queryClient.setQueryData(["mods", variables.gamePath], result?.previous);
		},
		onSettled: (_data, _error, variables) => {
			if (queryClient.isMutating({ mutationKey: ["mods-edit"] }) > 1) return;
			void queryClient.invalidateQueries({ queryKey: ["mods", variables.gamePath] });
		},
	}));
};

type ToggleVariables = { gamePath: string; modName: string };

export const createEnableModMutation = () =>
	createOptimisticModMutation<ToggleVariables>(
		({ gamePath, modName }) => enableMod(gamePath, modName),
		(mod, { modName }) => (mod.Name === modName ? { ...mod, Enabled: true } : mod),
	);

export const createDisableModMutation = () =>
	createOptimisticModMutation<ToggleVariables>(
		({ gamePath, modName }) => disableMod(gamePath, modName),
		(mod, { modName }) => (mod.Name === modName ? { ...mod, Enabled: false } : mod),
	);

export const createRenameModMutation = () =>
	createOptimisticModMutation<{ gamePath: string; oldName: string; newName: string }>(
		({ gamePath, oldName, newName }) => renameMod(gamePath, oldName, newName),
		(mod, { oldName, newName }) => (mod.Name === oldName ? { ...mod, Name: newName } : mod),
	);
