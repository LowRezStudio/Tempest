import fs from "node:fs";
import path from "node:path";
import { shell } from "electron";
import { handle } from "./ipc/handle.js";

// openPath executes files and openExternal hands any scheme to the OS, so only
// folders, the schemes the renderer links can use and the Windows Security page
// are allowed through.
const ALLOWED_EXTERNAL_SCHEMES = new Set(["http:", "https:", "mailto:", "tel:"]);
const ALLOWED_EXTERNAL_URLS = new Set(["windowsdefender://threatsettings"]);

handle("opener:open-path", async (_event, { path: target }) => {
	const resolved = path.resolve(target);
	if (!fs.statSync(resolved, { throwIfNoEntry: false })?.isDirectory()) {
		throw new Error("Only folders can be opened");
	}
	const error = await shell.openPath(resolved);
	if (error) throw new Error(error);
});

handle("opener:open-url", (_event, { url }) => {
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		throw new Error("Invalid URL");
	}
	if (!ALLOWED_EXTERNAL_SCHEMES.has(parsed.protocol) && !ALLOWED_EXTERNAL_URLS.has(parsed.href)) {
		throw new Error(`Refusing to open a ${parsed.protocol} URL`);
	}
	void shell.openExternal(parsed.href);
});
