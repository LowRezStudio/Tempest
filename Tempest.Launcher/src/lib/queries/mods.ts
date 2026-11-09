import { createMutation, createQueries, createQuery, useQueryClient } from "@tanstack/svelte-query";
import { disableMod, enableMod, installMod, listMods, removeMod } from "$lib/core/mods";
import { trackInstall } from "$lib/mods/pendingInstalls.svelte";
import { invoke } from "$lib/platform/api";
import type { ModInstallResult, ModRecord, PatchProgress } from "$lib/core/mods";

export const createModsQuery = (instancePath: () => string) =>
	createQuery(() => {
		const pathValue = instancePath();
		return {
			queryKey: ["mods", pathValue],
			queryFn: () => listMods(pathValue),
			enabled: !!pathValue,
		};
	});

/**
 * Several instances' mod lists keyed by game path, sharing the cache with createModsQuery.
 * The record sits under one key: svelte-query tracks the combined result by its top-level keys,
 * so paths added later would never update.
 * @param instancePaths - The game paths to list.
 * @returns Each path's mods in `byPath`, undefined until loaded.
 */
export const createModsQueries = (instancePaths: () => string[]) =>
	createQueries(() => {
		const paths = instancePaths();
		return {
			queries: paths.map((pathValue) => ({
				queryKey: ["mods", pathValue],
				queryFn: () => listMods(pathValue),
			})),
			combine: (results) => ({
				byPath: Object.fromEntries(
					paths.map((pathValue, i) => [pathValue, results[i]?.data]),
				) as Record<string, ModRecord[] | undefined>,
			}),
		};
	});

/**
 * The icons installed V2 mods ship in their own package, as data URLs by mod id. Keyed under the
 * instance's mod list, so every install or edit that refreshes the list reloads them too.
 * @param instancePath - The game path the mods are installed in.
 * @param modIds - The installed V2 mods' ids.
 * @returns A map of mod id to icon; mods without an icon are left out.
 */
export const createInstalledIconsQuery = (instancePath: () => string, modIds: () => string[]) =>
	createQuery(() => {
		const pathValue = instancePath();
		const ids = [...modIds()].sort();
		return {
			queryKey: ["mods", pathValue, "icons", ids],
			queryFn: async () => {
				const icons = await Promise.all(
					ids.map((modId) =>
						invoke("mods:installed-icon", { gamePath: pathValue, modId }).catch(
							() => null,
						),
					),
				);
				return new Map(ids.flatMap((id, i) => (icons[i] ? [[id, icons[i]] as const] : [])));
			},
			enabled: !!pathValue && ids.length > 0,
		};
	});

/** Reads existing per-mod config files for installed V2 mods. */
export const createModConfigsQuery = (instancePath: () => string, modIds: () => string[]) =>
	createQuery(() => {
		const pathValue = instancePath();
		const ids = [...modIds()].sort();
		return {
			queryKey: ["mods", pathValue, "configs", ids],
			queryFn: async () => {
				const configs = await Promise.all(
					ids.map((modId) =>
						invoke("mods:read-config", { gamePath: pathValue, modId }).catch(
							() => null,
						),
					),
				);
				return new Map(
					ids.flatMap((id, i) =>
						configs[i] !== null ? [[id, configs[i]!] as const] : [],
					),
				);
			},
			enabled: !!pathValue && ids.length > 0,
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

// Keep the last confirmed state while the row is busy. Refresh on failure too: restoring
// an optimistic snapshot of the entire list can undo another mod's successful change.
const createModMutation = <V extends ModNameVariables>(
	run: (variables: V, onProgress: PatchProgress) => Promise<ModInstallResult>,
) => {
	const queryClient = useQueryClient();
	return createMutation(() => ({
		mutationKey: ["mods-edit"],
		mutationFn: async (variables: V) => {
			const { gamePath, modName } = variables;
			const modId = queryClient
				.getQueryData<ModRecord[]>(["mods", gamePath])
				?.find((mod) => mod.Name === modName)?.Id;
			const tracker = trackInstall(gamePath, {
				name: modName,
				modId,
				inPlace: true,
			});
			try {
				const res = await run(variables, tracker.setPatching);
				if (!res.Success) throw new Error(res.Message || "Unknown error");
				return res;
			} finally {
				await tracker.finish(true);
			}
		},
	}));
};

type ModNameVariables = { gamePath: string; modName: string };

export const createEnableModMutation = () =>
	createModMutation<ModNameVariables>(({ gamePath, modName }, onProgress) =>
		enableMod(gamePath, modName, onProgress),
	);

export const createDisableModMutation = () =>
	createModMutation<ModNameVariables>(({ gamePath, modName }, onProgress) =>
		disableMod(gamePath, modName, onProgress),
	);

export const createRemoveModMutation = () =>
	createModMutation<ModNameVariables>(({ gamePath, modName }, onProgress) =>
		removeMod(gamePath, modName, onProgress),
	);
