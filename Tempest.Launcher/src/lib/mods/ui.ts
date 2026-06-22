import { atom } from "nanostores";

let activeResolver: ((value: boolean) => void) | undefined = undefined;

export const replaceDialogStore = atom<{ open: boolean; modName: string; isModConflict: boolean }>({
	open: false,
	modName: "",
	isModConflict: true,
});

replaceDialogStore.listen((state) => {
	if (!state.open && activeResolver) {
		const resolve = activeResolver;
		activeResolver = undefined;
		resolve(false);
	}
});

export const confirmReplaceMod = (modName: string, isModConflict = true): Promise<boolean> => {
	if (!isModConflict) {
		return Promise.resolve(true);
	}
	return new Promise<boolean>((resolve) => {
		activeResolver = resolve;
		replaceDialogStore.set({
			open: true,
			modName,
			isModConflict,
		});
	});
};

export const resolveReplaceMod = (value: boolean) => {
	const resolve = activeResolver;
	activeResolver = undefined;
	replaceDialogStore.set({ open: false, modName: "", isModConflict: true });
	resolve?.(value);
};

let activeUnverifiedResolver: ((value: boolean) => void) | undefined = undefined;

export const unverifiedDialogStore = atom<{ open: boolean; modName: string }>({
	open: false,
	modName: "",
});

unverifiedDialogStore.listen((state) => {
	if (!state.open && activeUnverifiedResolver) {
		const resolve = activeUnverifiedResolver;
		activeUnverifiedResolver = undefined;
		resolve(false);
	}
});

export const confirmUnverifiedMod = (modName: string): Promise<boolean> => {
	return new Promise<boolean>((resolve) => {
		activeUnverifiedResolver = resolve;
		unverifiedDialogStore.set({
			open: true,
			modName,
		});
	});
};

export const resolveUnverifiedMod = (value: boolean) => {
	const resolve = activeUnverifiedResolver;
	activeUnverifiedResolver = undefined;
	unverifiedDialogStore.set({ open: false, modName: "" });
	resolve?.(value);
};
