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
