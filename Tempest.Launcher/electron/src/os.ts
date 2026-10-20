import os from "node:os";
import type { OsInfo } from "./ipc/contract.js";
import type { BrowserWindow } from "electron";

const osInfo = (): OsInfo => ({
	platform:
		process.platform === "win32"
			? "windows"
			: process.platform === "darwin"
				? "macos"
				: "linux",
	arch: process.arch === "x64" ? "x86_64" : process.arch === "arm64" ? "aarch64" : process.arch,
	type:
		process.platform === "win32"
			? "Windows_NT"
			: process.platform === "darwin"
				? "Darwin"
				: "Linux",
	version: os.release(),
});

export function injectOs(window: BrowserWindow): void {
	const payload = JSON.stringify(osInfo());
	// Overwrite the provisional value the preload script exposed (it lacks the
	// OS version, which a sandboxed preload cannot read). Plain assignment: the
	// preload property is writable, unlike the old non-configurable definition.
	void window.webContents
		.executeJavaScript(`window.__os = Object.freeze(${payload});void 0`)
		.catch(() => {});
}
