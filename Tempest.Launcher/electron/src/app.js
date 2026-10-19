import { ipcMain, app } from "electron";

let pendingOpenFiles = [];
let pendingDeepLinks = [];

export function queueOpenFiles(files) {
	pendingOpenFiles.push(...files);
}

export function queueDeepLinks(urls) {
	pendingDeepLinks.push(...urls);
}

ipcMain.handle("app:version", () => app.getVersion());
ipcMain.handle("take_pending_open_files", () => {
	const files = pendingOpenFiles;
	pendingOpenFiles = [];
	return files;
});
ipcMain.handle("take_pending_deep_links", () => {
	const urls = pendingDeepLinks;
	pendingDeepLinks = [];
	return urls;
});
ipcMain.handle("relaunch", () => {
	app.relaunch();
	app.exit();
});
