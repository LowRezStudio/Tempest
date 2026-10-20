import { BrowserWindow } from "electron";
import { handle } from "./ipc/handle.js";

const windowFrom = (event: Electron.IpcMainInvokeEvent) =>
	BrowserWindow.fromWebContents(event.sender);

handle("window:destroy", (event) => {
	windowFrom(event)?.destroy();
});

handle("window:hide", (event) => {
	windowFrom(event)?.hide();
});

handle("window:show", (event) => {
	windowFrom(event)?.show();
});

handle("window:set-focus", (event) => {
	windowFrom(event)?.focus();
});

handle("window:unminimize", (event) => {
	windowFrom(event)?.restore();
});
