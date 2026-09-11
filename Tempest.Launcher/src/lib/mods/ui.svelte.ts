export type ConflictModInfo = {
	ModId: string;
	ModName: string;
	ModVersion?: string;
	ConflictingFiles: string[];
};

export const replaceDialogStore = $state({
	value: {
		open: false,
		modName: "",
		newModName: "",
		conflictingMods: [] as ConflictModInfo[],
		isModConflict: true,
	},
});

let activeResolver: ((value: boolean) => void) | undefined = undefined;

export const confirmReplaceMod = (
	modName: string,
	isModConflict = true,
	conflictingMods: ConflictModInfo[] = [],
	newModName?: string,
): Promise<boolean> => {
	if (!isModConflict) {
		return Promise.resolve(true);
	}
	return new Promise<boolean>((resolve) => {
		replaceDialogStore.value = {
			open: true,
			modName,
			newModName: newModName ?? modName,
			conflictingMods,
			isModConflict,
		};
		activeResolver = resolve;
	});
};

export const resolveReplaceMod = (value: boolean) => {
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
	value: { open: false, modName: "" },
});

let activeUnverifiedResolver: ((value: boolean) => void) | undefined = undefined;

export const confirmUnverifiedMod = (modName: string): Promise<boolean> => {
	return new Promise<boolean>((resolve) => {
		unverifiedDialogStore.value = {
			open: true,
			modName,
		};
		activeUnverifiedResolver = resolve;
	});
};

export const resolveUnverifiedMod = (value: boolean) => {
	const resolve = activeUnverifiedResolver;
	activeUnverifiedResolver = undefined;
	unverifiedDialogStore.value = { open: false, modName: "" };
	resolve?.(value);
};
