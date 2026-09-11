import { goto } from "$app/navigation";
import { page } from "$app/state";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { installMod } from "$lib/core/mods";
import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui.svelte";
import { m } from "$lib/paraglide/messages";
import { instanceMap } from "$lib/stores/instance.svelte";
import { addToast, removeToast } from "$lib/stores/ui.svelte";

export const isDraggingFiles = $state({ value: false });
export const isDraggingConverterFiles = $state({ value: false });
export const showInstanceSelect = $state({ value: false });
export const converterPendingPaths = $state<string[]>([]);

let onModsInstalled: ((path: string) => void) | undefined;
export function setOnModsInstalled(fn: (path: string) => void) {
	onModsInstalled = fn;
}

let droppedFilePaths = $state<string[]>([]);
let installQueue = Promise.resolve();

function queueInstall(filePaths: string[], instance: any) {
	const queuedPaths = [...filePaths];
	installQueue = installQueue
		.then(() => proceedWithInstall(instance, queuedPaths))
		.catch((error: unknown) => {
			console.error("Mod install queue failed:", error);
			addToast({
				title: m.toast_installation_failed_title(),
				message:
					error instanceof Error ? error.message : m.toast_installation_failed_internal(),
				tone: "error",
			});
		});
}

function handleModFileDrop(filePaths: string[]) {
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

	const pathname = page.url.pathname;
	const match = pathname.match(/^\/instance\/([^/]+)/);
	if (match) {
		const instanceId = match[1];
		const inst = instanceMap.value[instanceId];
		if (inst && inst.state?.type === "prepared") {
			queueInstall(validPaths, inst);
			return;
		}
	}

	// More file-open events may arrive while the instance picker is already open.
	// Preserve every file so selecting an instance installs the complete batch.
	droppedFilePaths = [...droppedFilePaths, ...validPaths];
	showInstanceSelect.value = true;
}

async function importPendingOpenFiles() {
	try {
		const paths = await invoke<string[]>("take_pending_open_files");
		if (paths.length > 0) handleModFileDrop(paths);
	} catch (error: unknown) {
		console.error("Failed to import pending open files:", error);
		addToast({
			title: m.toast_installation_failed_title(),
			message:
				error instanceof Error ? error.message : m.toast_installation_failed_internal(),
			tone: "error",
		});
	}
}

async function proceedWithInstall(instance: any, filePaths: string[]) {
	if (!instance || filePaths.length === 0) return;

	void goto(`/instance/${instance.id}`);

	let successCount = 0;
	let lastInstalledName = "";

	for (const filePath of filePaths) {
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
			let res = await installMod(instance.path, filePath, false, false);
			if (res.Unverified) {
				const confirmed = await confirmUnverifiedMod(modFileName);
				if (confirmed) {
					allowedUnsigned = true;
					res = await installMod(instance.path, filePath, false, true);
				} else {
					if (installingToastId) removeToast(installingToastId);
					continue;
				}
			}

			if (res.Conflict) {
				const confirmed = await confirmReplaceMod(
					modFileName,
					res.IsModConflict,
					res.ConflictingMods ?? [],
					res.NewModName ?? modFileName,
				);
				if (confirmed) {
					res = await installMod(instance.path, filePath, true, allowedUnsigned);
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
		onModsInstalled?.(instance.path);
	}
}

export function handleInstanceSelected(inst: any) {
	const filePaths = droppedFilePaths;
	droppedFilePaths = [];
	showInstanceSelect.value = false;
	queueInstall(filePaths, inst);
}

$effect.root(() => {
	let unlistenDrop: (() => void) | undefined;
	let unlistenOpenFiles: (() => void) | undefined;
	const appWindow = getCurrentWindow();
	void listen("open-mod-files", () => {
		void importPendingOpenFiles();
	}).then((fn) => {
		unlistenOpenFiles = fn;
		void importPendingOpenFiles();
	});
	void appWindow
		.onDragDropEvent((event) => {
			const onConverter = page.url.pathname === "/converter";
			if (event.payload.type === "enter" || event.payload.type === "over") {
				if (onConverter) {
					isDraggingConverterFiles.value = true;
				} else {
					isDraggingFiles.value = true;
				}
			} else if (event.payload.type === "drop") {
				isDraggingConverterFiles.value = false;
				isDraggingFiles.value = false;
				const paths = event.payload.paths;
				if (paths && paths.length > 0) {
					if (onConverter) {
						converterPendingPaths.push(...paths);
					} else {
						void handleModFileDrop(paths);
					}
				}
			} else if (event.payload.type === "leave") {
				isDraggingConverterFiles.value = false;
				isDraggingFiles.value = false;
			}
		})
		.then((fn) => {
			unlistenDrop = fn;
		});
	return () => {
		unlistenDrop?.();
		unlistenOpenFiles?.();
	};
});
