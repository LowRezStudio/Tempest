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

export async function invoke<K extends IpcChannel>(
	channel: K,
	...args: IpcContract[K]["args"] extends void ? [] : [IpcContract[K]["args"]]
): Promise<IpcContract[K]["result"]> {
	try {
		return await bridge().invoke(channel, ...args);
	} catch (error) {
		// Electron wraps rejections as "Error invoking remote method 'x': Error: msg".
		const message = error instanceof Error ? error.message : String(error);
		throw new Error(message.replace(/^Error invoking remote method '[^']*': (?:Error: )?/, ""));
	}
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

// Dialogs remember the last folder used per purpose ("import", "export",
// "package", …) so replacing or exporting several assets in a row stays in
// the same place.
const DIR_KEY = "studio.lastDir.";

export function lastDir(memory: string): string | undefined {
	try {
		return localStorage.getItem(DIR_KEY + memory) ?? undefined;
	} catch {
		return undefined;
	}
}

export function rememberDir(memory: string, filePath: string): void {
	const i = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
	if (i <= 0) return;
	try {
		localStorage.setItem(DIR_KEY + memory, filePath.slice(0, i));
	} catch {
		// ignore
	}
}

const hasDir = (p: string) => p.includes("/") || p.includes("\\");

export async function pickDirectory(
	title: string,
	defaultPath?: string,
	memory = "folder",
): Promise<string | null> {
	const r = await invoke("dialog:open", {
		directory: true,
		title,
		defaultPath: defaultPath ?? lastDir(memory),
	});
	const dir = r?.[0] ?? null;
	if (dir) rememberDir(memory, `${dir}/x`);
	return dir;
}

export async function pickFile(
	title: string,
	filters: DialogFilter[],
	defaultPath?: string,
	memory = "import",
): Promise<string | null> {
	const r = await invoke("dialog:open", {
		title,
		filters,
		defaultPath: defaultPath ?? lastDir(memory),
	});
	const file = r?.[0] ?? null;
	if (file) rememberDir(memory, file);
	return file;
}

export async function pickFiles(
	title: string,
	filters: DialogFilter[],
	memory = "import",
): Promise<string[]> {
	const files =
		(await invoke("dialog:open", {
			title,
			filters,
			multiple: true,
			defaultPath: lastDir(memory),
		})) ?? [];
	if (files[0]) rememberDir(memory, files[0]);
	return files;
}

/** `defaultPath` may be a bare file name: it then lands in the remembered folder. */
export async function pickSave(
	title: string,
	defaultPath: string,
	filters: DialogFilter[],
	memory = "export",
): Promise<string | null> {
	const dir = lastDir(memory);
	const start = !hasDir(defaultPath) && dir ? `${dir}/${defaultPath}` : defaultPath;
	const out = await invoke("dialog:save", { title, defaultPath: start, filters });
	if (out) rememberDir(memory, out);
	return out;
}
