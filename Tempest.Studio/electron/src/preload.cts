// Sandboxed preload: exposes the typed IPC bridge and OS facts. CJS (.cts)
// because sandboxed preloads cannot load ESM.
import { contextBridge, ipcRenderer, webUtils } from "electron";
import type { PreloadApi } from "./ipc/contract.js";

const platform =
	process.platform === "win32" ? "windows" : process.platform === "darwin" ? "macos" : "linux";
const arch =
	process.arch === "x64" ? "x86_64" : process.arch === "arm64" ? "aarch64" : process.arch;

contextBridge.exposeInMainWorld("__os", { platform, arch });

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
