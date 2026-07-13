const { contextBridge, ipcRenderer, webUtils } = require("electron");

contextBridge.exposeInMainWorld("electronAPI", {
	invoke: (channel, ...args) => ipcRenderer.invoke(channel, ...args),
	on: (channel, callback) => {
		const fn = (_event, ...args) => callback(...args);
		ipcRenderer.on(channel, fn);
		return () => ipcRenderer.removeListener(channel, fn);
	},
	getPathForFile: (file) => webUtils.getPathForFile(file),
});
