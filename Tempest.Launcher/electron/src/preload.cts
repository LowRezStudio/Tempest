// Sandboxed preload: exposes the typed IPC bridge plus pre-page-load OS info
// and the Tauri-era localStorage migration dump. CJS (.cts) because sandboxed
// preloads cannot load ESM.
import { contextBridge, ipcRenderer, webUtils } from "electron";
import type { PreloadApi } from "./ipc/contract.js";

// OS info, available before any page script runs (module init and component
// mounts read it via platform()/arch()). The main process refines `version`
// after load; sandboxed preloads have process.platform/process.arch but no os.
const platform =
	process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
const type =
	process.platform === "win32"
		? "Windows_NT"
		: process.platform === "darwin"
			? "Darwin"
			: "Linux";
const arch =
	process.arch === "x64" ? "x86_64" : process.arch === "arm64" ? "aarch64" : process.arch;

// localStorage dump from the previous (Tauri) profile, consumed by the store
// bootstrap before any persisted state initializes.
contextBridge.exposeInMainWorld("__migrationDump", ipcRenderer.sendSync("migration:dump"));
// Host-held renderer state (see electron/src/state.ts): read synchronously so
// stores start with it after a page reload. Missing on an older host build.
let hostState: Record<string, unknown> = {};
try {
	hostState = ipcRenderer.sendSync("state:get-sync") ?? {};
} catch {
	/* host predates the channel: stores fall back to their defaults */
}
contextBridge.exposeInMainWorld("__hostState", hostState);

contextBridge.exposeInMainWorld("__os", { platform, arch, type, version: "" });

// Channels are not filtered here on purpose: the bridge is only reachable from
// our own renderer (contextIsolation + sandbox + navigation lockout), and the
// channel/payload types are enforced at compile time via IpcContract.
const api: PreloadApi = {
	invoke: (channel, ...args) => ipcRenderer.invoke(channel, ...args),
	on: (channel, callback) => {
		const fn = (_event: unknown, ...args: unknown[]) =>
			callback(args[0] as Parameters<typeof callback>[0]);
		ipcRenderer.on(channel, fn);
		return () => {
			ipcRenderer.removeListener(channel, fn);
		};
	},
	getPathForFile: (file) => webUtils.getPathForFile(file),
};

contextBridge.exposeInMainWorld("electronAPI", api);
