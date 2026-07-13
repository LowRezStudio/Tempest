const { ipcMain } = require("electron");
const fs = require("node:fs");

ipcMain.handle("fs:exists", (_event, { path: p }) => fs.existsSync(p));

ipcMain.handle("fs:read-dir", (_event, { path: p }) => {
	return fs.readdirSync(p, { withFileTypes: true }).map((d) => ({
		name: d.name,
		isDirectory: d.isDirectory(),
		isFile: d.isFile(),
		isSymlink: d.isSymbolicLink(),
	}));
});

ipcMain.handle("fs:stat", (_event, { path: p }) => {
	const s = fs.statSync(p);
	return {
		size: s.size,
		isDirectory: s.isDirectory(),
		isFile: s.isFile(),
		isSymlink: s.isSymbolicLink(),
	};
});

ipcMain.handle("fs:remove", (_event, { path: p, options }) => fs.rmSync(p, options ?? {}));
