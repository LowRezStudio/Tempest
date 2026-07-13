const { ipcMain, dialog, BrowserWindow } = require("electron");

ipcMain.handle("dialog:open", async (event, { options }) => {
	const win = BrowserWindow.fromWebContents(event.sender);
	const result = await dialog.showOpenDialog(win, {
		title: options?.title,
		properties: [
			...(options?.directory ? ["openDirectory"] : ["openFile"]),
			...(options?.multiple ? ["multiSelections"] : []),
		],
		filters: options?.filters,
	});
	if (result.canceled) return null;
	return options?.multiple ? result.filePaths : (result.filePaths[0] ?? null);
});
