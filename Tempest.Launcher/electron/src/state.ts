// Renderer state parked in the host. The main process lives as long as the
// app, so a page reload (Vite full reload, renderer crash) re-reads this
// instead of starting over. In memory on purpose: quitting the app clears it.
import { ipcMain } from "electron";
import { handle } from "./ipc/handle.js";

const state: Record<string, unknown> = {};

handle("state:set", (_event, { patch }) => {
	Object.assign(state, patch);
});

// Synchronous read for the preload script: it runs before any page code, so
// the renderer's stores can start with the host's value instead of a default.
ipcMain.on("state:get-sync", (event) => {
	event.returnValue = state;
});
