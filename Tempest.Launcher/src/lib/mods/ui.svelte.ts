import type { DependencyToInstall } from "$lib/mods/dependencies";
import type { RepoCompat } from "$lib/queries/repo";
import type { Instance } from "$lib/types/instance";

export type ConflictModInfo = {
	ModId: string;
	ModName: string;
	ModVersion?: string;
	ConflictingFiles: string[];
};

export type ReplaceModChoice = "cancel" | "replace" | "stack";

// Installs run side by side but ask their questions one at a time.
let dialogQueue: Promise<unknown> = Promise.resolve();
function nextDialog<T>(open: () => Promise<T>): Promise<T> {
	const shown = dialogQueue.then(open);
	dialogQueue = shown;
	return shown;
}

export const replaceDialogStore = $state({
	value: {
		open: false,
		modName: "",
		newModName: "",
		conflictingMods: [] as ConflictModInfo[],
		isModConflict: true,
	},
});

let activeResolver: ((value: ReplaceModChoice) => void) | undefined = undefined;

export const confirmReplaceMod = (
	modName: string,
	isModConflict = true,
	conflictingMods: ConflictModInfo[] = [],
	newModName?: string,
): Promise<ReplaceModChoice> => {
	if (!isModConflict) {
		return Promise.resolve("replace");
	}
	return nextDialog(
		() =>
			new Promise<ReplaceModChoice>((resolve) => {
				replaceDialogStore.value = {
					open: true,
					modName,
					newModName: newModName ?? modName,
					conflictingMods,
					isModConflict,
				};
				activeResolver = resolve;
			}),
	);
};

export const resolveReplaceMod = (value: ReplaceModChoice) => {
	const resolve = activeResolver;
	activeResolver = undefined;
	replaceDialogStore.value = {
		open: false,
		modName: "",
		newModName: "",
		conflictingMods: [],
		isModConflict: true,
	};
	resolve?.(value);
};

export const unverifiedDialogStore = $state({
	value: { open: false, modName: "", signer: "", keyListOffline: false },
});

let activeUnverifiedResolver: ((value: boolean) => void) | undefined = undefined;

export const confirmUnverifiedMod = (
	modName: string,
	signer: string,
	keyListOffline: boolean,
): Promise<boolean> => {
	return nextDialog(
		() =>
			new Promise<boolean>((resolve) => {
				unverifiedDialogStore.value = {
					open: true,
					modName,
					signer,
					keyListOffline,
				};
				activeUnverifiedResolver = resolve;
			}),
	);
};

export const resolveUnverifiedMod = (value: boolean) => {
	const resolve = activeUnverifiedResolver;
	activeUnverifiedResolver = undefined;
	unverifiedDialogStore.value = { open: false, modName: "", signer: "", keyListOffline: false };
	resolve?.(value);
};

export type DependenciesChoice = "install" | "skip" | "cancel";

export const dependenciesDialogStore = $state({
	value: { open: false, modName: "", items: [] as DependencyToInstall[] },
});

let activeDependenciesResolver: ((value: DependenciesChoice) => void) | undefined = undefined;

// "install" fetches the listed mods first, "skip" installs the mod alone, "cancel" stops.
export const confirmDependencies = (
	modName: string,
	items: DependencyToInstall[],
): Promise<DependenciesChoice> => {
	return nextDialog(
		() =>
			new Promise<DependenciesChoice>((resolve) => {
				dependenciesDialogStore.value = { open: true, modName, items };
				activeDependenciesResolver = resolve;
			}),
	);
};

export const resolveDependencies = (value: DependenciesChoice) => {
	const resolve = activeDependenciesResolver;
	activeDependenciesResolver = undefined;
	dependenciesDialogStore.value = { open: false, modName: "", items: [] };
	resolve?.(value);
};

export type InstancePickerRequest = { compat: RepoCompat | null; hint?: string };

export const instancePickerStore = $state({
	value: { open: false, request: null as InstancePickerRequest | null },
});

let activePickerResolver: ((value: Instance | null) => void) | undefined = undefined;

// Resolves to null when the picker is dismissed.
export const pickCompatibleInstance = (
	request: InstancePickerRequest,
): Promise<Instance | null> => {
	activePickerResolver?.(null);
	return new Promise<Instance | null>((resolve) => {
		instancePickerStore.value = { open: true, request };
		activePickerResolver = resolve;
	});
};

export const resolvePickInstance = (value: Instance | null) => {
	const resolve = activePickerResolver;
	activePickerResolver = undefined;
	instancePickerStore.value = { open: false, request: instancePickerStore.value.request };
	resolve?.(value);
};
