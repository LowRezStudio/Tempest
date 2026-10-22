// Electron main entry of Tempest Studio: one window, the `upk serve`
// backend, file associations for .upk files. IPC handlers live in their own
// modules and register themselves when imported.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, session } from "electron";
import serve from "electron-serve";
import "./dialog.js";
import "./system.js";
import { installSmoke } from "./smoke.js";
import { upk } from "./upk.js";

const DEV_URL = "http://localhost:1421";

let mainWindow: BrowserWindow | null = null;
let pendingFiles: string[] = [];

function upkFilesFromArgs(args: string[], workingDirectory: string): string[] {
	return args.flatMap((argument) => {
		let filePath: string;
		try {
			filePath = argument.startsWith("file:")
				? fileURLToPath(argument)
				: path.isAbsolute(argument)
					? argument
					: path.resolve(workingDirectory, argument);
		} catch {
			return [];
		}
		const ext = path.extname(filePath).toLowerCase();
		return (ext === ".upk" || ext === ".u") && fs.existsSync(filePath) ? [filePath] : [];
	});
}

function queueOpenFiles(files: string[]): void {
	if (files.length === 0) return;
	pendingFiles.push(...files);
	if (mainWindow && !mainWindow.isDestroyed() && !mainWindow.webContents.isLoading()) {
		mainWindow.webContents.send("open-files", pendingFiles);
		pendingFiles = [];
	}
}

queueOpenFiles(upkFilesFromArgs(process.argv.slice(1), process.cwd()));

app.on("open-file", (event, filePath) => {
	event.preventDefault();
	queueOpenFiles(upkFilesFromArgs([filePath], process.cwd()));
});

const hasInstanceLock = app.requestSingleInstanceLock();
if (!hasInstanceLock) {
	app.quit();
} else {
	app.on("second-instance", (_event, commandLine, workingDirectory) => {
		queueOpenFiles(upkFilesFromArgs(commandLine, workingDirectory));
		if (mainWindow) {
			if (mainWindow.isMinimized()) mainWindow.restore();
			mainWindow.show();
			mainWindow.focus();
		}
	});
}

const loadURL = serve({ directory: "build" });

function createWindow(): void {
	const icon = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "resources", "icons", "icon.png");

	const window = new BrowserWindow({
		width: 1600,
		height: 950,
		minWidth: 1100,
		minHeight: 700,
		title: "Tempest Studio",
		icon,
		autoHideMenuBar: true,
		backgroundColor: "#1e1e2e",
		webPreferences: {
			preload: path.join(import.meta.dirname, "preload.cjs"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			devTools: true,
		},
	});
	mainWindow = window;
	if (!app.isPackaged) installSmoke(window);

	window.webContents.on("did-finish-load", () => {
		if (pendingFiles.length > 0) {
			window.webContents.send("open-files", pendingFiles);
			pendingFiles = [];
		}
	});

	window.webContents.on("will-navigate", (event, url) => {
		try {
			const u = new URL(url);
			const sameOrigin = app.isPackaged
				? u.protocol === "app:"
				: `${u.protocol}//${u.host}` === DEV_URL;
			if (!sameOrigin) event.preventDefault();
		} catch {
			event.preventDefault();
		}
	});
	window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));

	if (!app.isPackaged) {
		void window.loadURL(DEV_URL);
	} else {
		void loadURL(window);
	}

	window.on("closed", () => {
		mainWindow = null;
	});
}

app.whenReady()
	.then(() => {
		if (!hasInstanceLock) return;
		// Deny every permission request (camera, notifications, ...): the app needs none.
		session.defaultSession.setPermissionRequestHandler((_wc, _permission, callback) =>
			callback(false),
		);
		upk.start();
		createWindow();
		app.on("activate", () => {
			if (BrowserWindow.getAllWindows().length === 0) createWindow();
		});
	})
	.catch((error: unknown) => {
		console.error(error);
		app.quit();
	});

app.on("window-all-closed", () => {
	app.quit();
});
