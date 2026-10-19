const { contextBridge, ipcRenderer, webUtils } = require("electron");

// localStorage dump from the previous (Tauri) profile, consumed by the store
// bootstrap before any persisted state initializes.
contextBridge.exposeInMainWorld("__migrationDump", ipcRenderer.sendSync("migration:dump"));

contextBridge.exposeInMainWorld("electronAPI", {
	invoke: (channel, ...args) => ipcRenderer.invoke(channel, ...args),
	on: (channel, callback) => {
		const fn = (_event, ...args) => callback(...args);
		ipcRenderer.on(channel, fn);
		return () => ipcRenderer.removeListener(channel, fn);
	},
	getPathForFile: (file) => webUtils.getPathForFile(file),
});
