import fs from "node:fs";
import path from "node:path";
import { shell } from "electron";
import { handle } from "./ipc/handle.js";

// shell.openPath runs files with their default handler (an .exe, .bat, .lnk or
// .url is executed) and shell.openExternal hands any scheme to the OS (file:,
// ms-msdt:, search-ms:, ...). The renderer only ever opens folders, web pages,
// and the Windows Security threat settings page.
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
	const isHttp = parsed.protocol === "https:" || parsed.protocol === "http:";
	if (!isHttp && !ALLOWED_EXTERNAL_URLS.has(parsed.href)) {
		throw new Error(`Refusing to open a ${parsed.protocol} URL`);
	}
	void shell.openExternal(parsed.href);
});
