// Small host-side helpers: app facts, filesystem checks and reveal/open in
// the system file manager. The renderer never touches the filesystem itself.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { app, BrowserWindow, shell } from "electron";
import { handle } from "./ipc/handle.js";

handle("app:version", () => app.getVersion());
handle("app:home", () => os.homedir());
handle("app:documents", () => app.getPath("documents"));
handle("app:temp-path", (_event, { name }) => {
	const dir = path.join(app.getPath("temp"), "tempest-studio");
	fs.mkdirSync(dir, { recursive: true });
	return path.join(dir, `${Date.now()}-${path.basename(name)}`);
});

handle("window:set-title", (event, { title }) => {
	BrowserWindow.fromWebContents(event.sender)?.setTitle(title);
});

handle("fs:exists", (_event, { path: p }) => fs.existsSync(p));
handle("fs:read-text", (_event, { path: p }) => fs.promises.readFile(p, "utf8"));
handle("fs:write-text", (_event, { path: p, text }) => fs.promises.writeFile(p, text, "utf8"));
handle("fs:remove", (_event, { path: p }) => fs.promises.rm(p, { force: true, recursive: true }));
handle("fs:mkdir", async (_event, { path: p }) => {
	await fs.promises.mkdir(p, { recursive: true });
});
handle("fs:list", async (_event, { path: p }) => {
	const out: string[] = [];
	const walk = async (dir: string, rel: string): Promise<void> => {
		let entries: fs.Dirent[];
		try {
			entries = await fs.promises.readdir(dir, { withFileTypes: true });
		} catch {
			return;
		}
		for (const e of entries) {
			if (e.name.startsWith(".")) continue;
			const r = rel ? `${rel}/${e.name}` : e.name;
			if (e.isDirectory()) await walk(path.join(dir, e.name), r);
			else out.push(r);
		}
	};
	await walk(p, "");
	return out;
});

handle("shell:show-item", (_event, { path: p }) => shell.showItemInFolder(p));
handle("shell:open-path", (_event, { path: p }) => shell.openPath(p));
