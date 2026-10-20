import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, ipcMain, session } from "electron";
import serve from "electron-serve";
import { queueOpenFiles } from "./app.js";
import "./fs.js";
import "./dialog.js";
import "./opener.js";
import { injectOs } from "./os.js";
import "./path.js";
import { activeChildren } from "./shell.js";
import "./window.js";
import "./scopes.js";
import "./sql.js";
import "./updater.js";
import "./tray.js";
import "./legacy.js";

let mainWindow = null;

function tempestFilesFromArgs(args, workingDirectory) {
	return args.flatMap((argument) => {
		let filePath;
		try {
			// Cross-platform support for Linux, Windows, and macOS file paths.
			filePath = argument.startsWith("file:")
				? fileURLToPath(argument)
				: path.isAbsolute(argument)
					? argument
					: path.resolve(workingDirectory, argument);
		} catch {
			return [];
		}

		return path.extname(filePath).toLowerCase() === ".tempest" && fs.existsSync(filePath)
			? [filePath]
			: [];
	});
}

function queueAndNotifyOpenFiles(files) {
	if (files.length === 0) return;
	queueOpenFiles(files);
	if (mainWindow && !mainWindow.webContents.isLoading()) {
		mainWindow.webContents.send("open-mod-files", null);
	}
}

process.chdir(process.cwd());
queueOpenFiles(tempestFilesFromArgs(process.argv.slice(1), process.cwd()));

// One-time localStorage migration: the Tauri build periodically dumped
// localStorage to settings.json next to this profile. Hand it to the preload
// script (which exposes it before any page script runs) so the store bootstrap
// can import it before persisted state initializes.
ipcMain.on("migration:dump", (event) => {
	let value = null;
	try {
		value = fs.readFileSync(path.join(app.getPath("userData"), "settings.json"), "utf8");
	} catch {
		/* no snapshot yet */
	}
	event.returnValue = value;
});

app.on("open-file", (event, filePath) => {
	event.preventDefault();
	queueAndNotifyOpenFiles(tempestFilesFromArgs([filePath], process.cwd()));
});

// Pin the userData dir to the one the Tauri build used (com.lowrezstudio.tempest)
// so the SQLite DBs, instance files, settings.json snapshot and localStorage all
// carry over instead of starting over in a fresh profile.
if (process.platform === "linux") {
	app.setPath("userData", path.join(os.homedir(), ".local", "share", "com.lowrezstudio.tempest"));
} else {
	app.setPath("userData", path.join(app.getPath("appData"), "com.lowrezstudio.tempest"));
}

// A second instance shares the pinned userData profile but cannot acquire its
// localStorage LevelDB lock, so it would silently run with empty, ephemeral
// storage. Refuse to start and surface the existing window instead.
const hasInstanceLock = app.requestSingleInstanceLock();
if (!hasInstanceLock) {
	app.quit();
} else {
	app.on("second-instance", (_event, commandLine, workingDirectory) => {
		queueAndNotifyOpenFiles(tempestFilesFromArgs(commandLine, workingDirectory));
		if (!mainWindow) return;
		if (mainWindow.isMinimized()) mainWindow.restore();
		mainWindow.show();
		mainWindow.focus();
	});
}

const loadURL = serve({ directory: "build" });

function createWindow() {
	const icon = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "resources", "icons", "icon.png");

	mainWindow = new BrowserWindow({
		width: 1500,
		height: 900,
		minWidth: 1024,
		minHeight: 800,
		title: "Tempest",
		icon,
		autoHideMenuBar: true,
		webPreferences: {
			preload: path.join(import.meta.dirname, "preload.cjs"),
			contextIsolation: true,
			nodeIntegration: false,
			sandbox: true,
			devTools: true,
		},
	});

	mainWindow.webContents.on("did-finish-load", () => {
		injectOs(mainWindow);
	});

	mainWindow.webContents.on("will-navigate", (event, url) => {
		const allowed = app.isPackaged ? ["app://"] : ["http://localhost:1420"];
		if (!allowed.some((p) => url.startsWith(p))) event.preventDefault();
	});

	// Never hand webview window requests (Ctrl/Cmd/Shift+click, middle-click,
	// target="_blank") to the system browser. External links are opened
	// explicitly from the renderer via openUrl().
	mainWindow.webContents.setWindowOpenHandler(() => ({ action: "deny" }));

	if (!app.isPackaged) {
		void mainWindow.loadURL("http://localhost:1420");
	} else {
		void loadURL(mainWindow);
	}

	mainWindow.on("close", async (e) => {
		e.preventDefault();
		try {
			const shouldClose = await mainWindow.webContents.executeJavaScript(
				"window.__closeHook?.() ?? true",
			);
			if (shouldClose) mainWindow.destroy();
		} catch {
			mainWindow.destroy();
		}
	});
}

app.whenReady()
	.then(() => {
		if (!hasInstanceLock) return;
		const upsert = (obj, key, value) => {
			const lower = key.toLowerCase();
			for (const k of Object.keys(obj)) {
				if (k.toLowerCase() === lower) {
					obj[k] = value;
					return;
				}
			}
			obj[key] = value;
		};

		session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
			const { responseHeaders: h } = details;

			if (details.method === "OPTIONS") {
				upsert(h, "Access-Control-Allow-Origin", ["*"]);
				upsert(h, "Access-Control-Allow-Methods", ["POST, GET, OPTIONS"]);
				upsert(h, "Access-Control-Allow-Headers", ["*"]);
				callback({ responseHeaders: h, statusLine: "HTTP/1.1 204 No Content" });
				return;
			}

			upsert(h, "Content-Security-Policy", [
				"default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src * data: blob:; media-src *; font-src * data:; connect-src *; frame-src *;",
			]);
			upsert(h, "Access-Control-Allow-Origin", ["*"]);
			callback({ responseHeaders: h });
		});
		createWindow();
	})
	.catch(console.error);

app.on("window-all-closed", () => {
	app.quit();
});

app.on("will-quit", () => {
	for (const [, child] of activeChildren) {
		try {
			child.kill();
		} catch {
			/* ignore */
		}
	}
	activeChildren.clear();
});
