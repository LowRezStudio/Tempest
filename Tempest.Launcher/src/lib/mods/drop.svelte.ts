import { goto } from "$app/navigation";
import { page } from "$app/state";
import { installModFiles } from "$lib/mods/installFile";
import { m } from "$lib/paraglide/messages";
import { invoke, listen, onDragDropEvent } from "$lib/platform/api";
import { instanceMap } from "$lib/stores/instance.svelte";
import { addToast } from "$lib/stores/ui.svelte";

export const isDraggingFiles = $state({ value: false });
export const isDraggingConverterFiles = $state({ value: false });
export const showInstanceSelect = $state({ value: false });
export const converterPendingPaths = $state<string[]>([]);

let onModsInstalled: ((path: string) => void) | undefined;
export function setOnModsInstalled(fn: (path: string) => void) {
	onModsInstalled = fn;
}

let droppedFilePaths = $state<string[]>([]);

function startInstall(filePaths: string[], instance: any) {
	if (!instance || filePaths.length === 0) return;
	void goto(`/instance/${instance.id}`);
	installModFiles(instance.path, filePaths)
		.then((installed) => {
			if (installed > 0) onModsInstalled?.(instance.path);
		})
		.catch((error: unknown) => {
			console.error("Mod install failed:", error);
			addToast({
				title: m.toast_installation_failed_title(),
				message:
					error instanceof Error ? error.message : m.toast_installation_failed_internal(),
				tone: "error",
			});
		});
}

// The instance whose page is open, when it is ready to take mods.
export function currentPreparedInstance() {
	const match = page.url.pathname.match(/^\/instance\/([^/]+)/);
	const inst = match ? instanceMap.value[match[1]] : undefined;
	return inst?.state?.type === "prepared" ? inst : undefined;
}

export function handleModFileDrop(filePaths: string[]) {
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

	const inst = currentPreparedInstance();
	if (inst) {
		startInstall(validPaths, inst);
		return;
	}

	// More file-open events may arrive while the instance picker is already open.
	// Preserve every file so selecting an instance installs the complete batch.
	droppedFilePaths = [...droppedFilePaths, ...validPaths];
	showInstanceSelect.value = true;
}

async function importPendingOpenFiles() {
	try {
		const paths = await invoke("app:take-pending-open-files");
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

export function handleInstanceSelected(inst: any) {
	const filePaths = droppedFilePaths;
	droppedFilePaths = [];
	showInstanceSelect.value = false;
	startInstall(filePaths, inst);
}

$effect.root(() => {
	let unlistenDrop: (() => void) | undefined;
	let unlistenOpenFiles: (() => void) | undefined;
	void listen("open-mod-files", () => {
		void importPendingOpenFiles();
	}).then((fn) => {
		unlistenOpenFiles = fn;
		void importPendingOpenFiles();
	});
	void onDragDropEvent((event) => {
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
					handleModFileDrop(paths);
				}
			}
		} else if (event.payload.type === "leave") {
			isDraggingConverterFiles.value = false;
			isDraggingFiles.value = false;
		}
	}).then((fn) => {
		unlistenDrop = fn;
	});
	return () => {
		unlistenDrop?.();
		unlistenOpenFiles?.();
	};
});
