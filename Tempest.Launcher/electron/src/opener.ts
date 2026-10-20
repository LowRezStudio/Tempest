import { shell } from "electron";
import { handle } from "./ipc/handle.js";

handle("opener:open-path", async (_event, { path }) => {
	const error = await shell.openPath(path);
	if (error) throw new Error(error);
});

handle("opener:open-url", (_event, { url }) => {
	void shell.openExternal(url);
});
