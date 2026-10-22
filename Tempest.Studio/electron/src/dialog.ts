import { BrowserWindow, dialog } from "electron";
import { handle } from "./ipc/handle.js";

handle("dialog:open", async (event, options) => {
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showOpenDialog(win!, {
		title: options?.title,
		defaultPath: options?.defaultPath,
		properties: [
			...(options?.directory ? ["openDirectory" as const] : ["openFile" as const]),
			...(options?.multiple ? ["multiSelections" as const] : []),
			"createDirectory" as const,
		],
		filters: options?.filters,
	});
	if (result.canceled) return null;
	return result.filePaths;
});

handle("dialog:save", async (event, options) => {
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showSaveDialog(win!, {
		title: options?.title,
		defaultPath: options?.defaultPath,
		filters: options?.filters,
	});
	if (result.canceled) return null;
	return result.filePath ?? null;
});
