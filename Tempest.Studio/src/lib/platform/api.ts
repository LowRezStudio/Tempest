// Renderer-side platform API over the Electron main process (window.electronAPI).
// Every channel and payload is typed by the host's IPC contract.
import type {
	DialogFilter,
	IpcChannel,
	IpcContract,
	IpcEventChannel,
	IpcEvents,
	PreloadApi,
} from "../../../electron/src/ipc/contract";

const bridge = (): PreloadApi => {
	if (!window.electronAPI) {
		throw new Error("electronAPI unavailable (not running under Electron)");
	}
	return window.electronAPI;
};

export function invoke<K extends IpcChannel>(
	channel: K,
	...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
): Promise<IpcContract[K]["result"]> {
	return bridge().invoke(channel, ...args);
}

export function listen<K extends IpcEventChannel>(
	event: K,
	handler: (payload: IpcEvents[K]) => void,
): () => void {
	return bridge().on(event, handler);
}

export function pathForFile(file: File): string {
	return bridge().getPathForFile(file);
}

export const os = () => window.__os ?? { platform: "linux" as const, arch: "x86_64" };

export const PACKAGE_FILTERS: DialogFilter[] = [
	{ name: "Unreal packages", extensions: ["upk", "u"] },
];

export async function pickDirectory(title: string, defaultPath?: string): Promise<string | null> {
	const r = await invoke("dialog:open", { directory: true, title, defaultPath });
	return r?.[0] ?? null;
}

export async function pickFile(
	title: string,
	filters: DialogFilter[],
	defaultPath?: string,
): Promise<string | null> {
	const r = await invoke("dialog:open", { title, filters, defaultPath });
	return r?.[0] ?? null;
}

export async function pickFiles(title: string, filters: DialogFilter[]): Promise<string[]> {
	return (await invoke("dialog:open", { title, filters, multiple: true })) ?? [];
}

export function pickSave(
	title: string,
	defaultPath: string,
	filters: DialogFilter[],
): Promise<string | null> {
	return invoke("dialog:save", { title, defaultPath, filters });
}
