import fs from "node:fs";
import path from "node:path";
import { app } from "electron";
import { handle } from "./ipc/handle.js";

handle("path:join", (_event, { paths }) => path.join(...paths));
handle("path:dirname", (_event, { path: p }) => path.dirname(p));
handle("path:resolve-resource", (_event, { paths }) => {
	const r = path.join(process.resourcesPath, ...paths);
	if (!app.isPackaged && !fs.existsSync(r)) {
		const dev = path.join(import.meta.dirname, "../../resources", ...paths);
		if (fs.existsSync(dev)) return dev;
	}
	return r;
});
