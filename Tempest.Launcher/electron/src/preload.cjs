const { contextBridge, ipcRenderer, webUtils } = require("electron");

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

contextBridge.exposeInMainWorld("__os", { platform, arch, type, version: "" });

contextBridge.exposeInMainWorld("electronAPI", {
	invoke: (channel, ...args) => ipcRenderer.invoke(channel, ...args),
	on: (channel, callback) => {
		const fn = (_event, ...args) => callback(...args);
		ipcRenderer.on(channel, fn);
		return () => ipcRenderer.removeListener(channel, fn);
	},
	getPathForFile: (file) => webUtils.getPathForFile(file),
});
