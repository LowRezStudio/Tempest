import { ipcMain } from "electron";

ipcMain.handle("scopes_allow_directory", () => {});
ipcMain.handle("scopes_allow_file", () => {});
ipcMain.handle("scopes_forbid_file", () => {});
ipcMain.handle("trigger_child_cleanup", () => {});
