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
	// Overwrite the provisional value the preload script exposed (it lacks the
	// OS version, which a sandboxed preload cannot read). Plain assignment: the
	// preload property is writable, unlike the old non-configurable definition.
	void window.webContents
		.executeJavaScript(`window.__os = Object.freeze(${payload});void 0`)
		.catch(() => {});
}
