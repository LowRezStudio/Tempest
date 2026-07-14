import { page } from "$app/state";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { installMod } from "$lib/core/mods";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { instanceMap } from "$lib/stores/instance.svelte";
import { addToast, removeToast } from "$lib/stores/ui.svelte";

export const isDraggingFiles = $state({ value: false });
export const showInstanceSelect = $state({ value: false });

let onModsInstalled: ((path: string) => void) | undefined;
export function setOnModsInstalled(fn: (path: string) => void) {
	onModsInstalled = fn;
}

let droppedFilePaths = $state<string[]>([]);
let targetInstance = $state<any>(null);

async function handleModFileDrop(filePaths: string[]) {
	const validPaths: string[] = [];
	let hadInvalid = false;

	for (const filePath of filePaths) {
		const ext = filePath.split(".").pop()?.toLowerCase();
		if (ext === "upk" || ext === "pck" || ext === "zip" || ext === "tempest") {
			validPaths.push(filePath);
		} else {
			hadInvalid = true;
		}
	}

	if (hadInvalid) {
		addToast({
			title: m.toast_unsupported_file_title(),
			message: m.toast_unsupported_file_message(),
			tone: "error",
		});
	}

	if (validPaths.length === 0) return;

	droppedFilePaths = validPaths;

	const pathname = page.url.pathname;
	const match = pathname.match(/^\/instance\/([^/]+)/);
	if (match) {
		const instanceId = match[1];
		const inst = instanceMap.value[instanceId];
		if (inst && inst.state?.type === "prepared") {
			targetInstance = inst;
			await proceedWithInstall();
			return;
		}
	}

	showInstanceSelect.value = true;
}

async function proceedWithInstall() {
	if (!targetInstance || droppedFilePaths.length === 0) return;

	let successCount = 0;
	let lastInstalledName = "";

	for (const filePath of droppedFilePaths) {
		const modFileName = filePath.split(/[/\\]/).pop() ?? filePath;
		let installingToastId: string | undefined;
		try {
			installingToastId = addToast({
				title: m.toast_mod_installing_title(),
				message: m.toast_mod_installing_message({ name: modFileName }),
				tone: "info",
				duration: 0,
			});

			let allowedUnsigned = false;
			let res = await installMod(targetInstance.path, filePath, false, false);
			if (res.Unverified) {
				const confirmed = await confirmUnverifiedMod(modFileName);
				if (confirmed) {
					allowedUnsigned = true;
					res = await installMod(targetInstance.path, filePath, false, true);
				} else {
					if (installingToastId) removeToast(installingToastId);
					continue;
				}
			}

			if (res.Conflict) {
				const confirmed = await confirmReplaceMod(modFileName, res.IsModConflict);
				if (confirmed) {
					res = await installMod(targetInstance.path, filePath, true, allowedUnsigned);
				} else {
					if (installingToastId) removeToast(installingToastId);
					continue;
				}
			}

			if (installingToastId) removeToast(installingToastId);

			if (res.Success) {
				successCount++;
				lastInstalledName =
					res.Mod?.Name ?? modFileName ?? m.toast_mod_installed_fallback();
			} else {
				addToast({
					title: m.toast_installation_failed_title(),
					message: `${modFileName}: ${res.Message || m.toast_installation_failed_unknown()}`,
					tone: "error",
				});
			}
		} catch (error: any) {
			if (installingToastId) removeToast(installingToastId);
			addToast({
				title: m.toast_installation_failed_title(),
				message: `${modFileName}: ${error.message ?? m.toast_installation_failed_internal()}`,
				tone: "error",
			});
		}
	}

	if (successCount > 0) {
		if (successCount === 1) {
			addToast({
				title: m.toast_mod_installed_title(),
				message: m.toast_mod_installed_message({ name: lastInstalledName }),
				tone: "success",
			});
		} else {
			addToast({
				title: m.toast_mod_installed_title(),
				message: `Successfully installed ${successCount} mods`,
				tone: "success",
			});
		}
		onModsInstalled?.(targetInstance.path);
	}
}

export function handleInstanceSelected(inst: any) {
	targetInstance = inst;
	showInstanceSelect.value = false;
	void proceedWithInstall();
}

$effect.root(() => {
	let unlistenDrop: (() => void) | undefined;
	const appWindow = getCurrentWindow();
	void appWindow
		.onDragDropEvent((event) => {
			if (event.payload.type === "enter" || event.payload.type === "over") {
				isDraggingFiles.value = true;
			} else if (event.payload.type === "drop") {
				isDraggingFiles.value = false;
				const paths = event.payload.paths;
				if (paths && paths.length > 0) {
					void handleModFileDrop(paths);
				}
			} else if (event.payload.type === "leave") {
				isDraggingFiles.value = false;
			}
		})
		.then((fn) => {
			unlistenDrop = fn;
		});
	return () => unlistenDrop?.();
});
