const { ipcMain } = require("electron");
const os = require("node:os");

ipcMain.handle("os:platform", () => {
	switch (process.platform) {
		case "win32": {
			return "windows";
		}
		case "darwin": {
			return "macos";
		}
		default: {
			return "linux";
		}
	}
});

ipcMain.handle("os:arch", () => {
	const a = process.arch;
	return a === "x64" ? "x86_64" : a === "arm64" ? "aarch64" : a;
});

ipcMain.handle("os:type", () => {
	switch (process.platform) {
		case "win32": {
			return "Windows_NT";
		}
		case "darwin": {
			return "Darwin";
		}
		default: {
			return "Linux";
		}
	}
});

ipcMain.handle("os:version", () => os.release());
