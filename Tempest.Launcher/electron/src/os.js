import os from "node:os";

export function injectOs(window) {
	const platform =
		process.platform === "win32"
			? "windows"
			: process.platform === "darwin"
				? "macos"
				: "linux";

	const arch =
		process.arch === "x64" ? "x86_64" : process.arch === "arm64" ? "aarch64" : process.arch;

	const type =
		process.platform === "win32"
			? "Windows_NT"
			: process.platform === "darwin"
				? "Darwin"
				: "Linux";

	const version = os.release();

	const payload = JSON.stringify({ platform, arch, type, version });
	void window.webContents.executeJavaScript(
		`Object.defineProperty(window,'__os',{value:Object.freeze(${payload}),writable:!1,configurable:!1})`,
	);
}
