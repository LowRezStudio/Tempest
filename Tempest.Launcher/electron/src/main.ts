// Electron main entry: window lifecycle, single-instance lock, file
// associations and session hardening. All IPC handlers live in their own
// domain modules and are registered for their side effects via the imports
// below.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, session } from "electron";
import serve from "electron-serve";
import { getMainWindow, setMainWindow, showMainWindow } from "./app-state.js";
import { queueDeepLinks, queueOpenFiles } from "./app.js";
import { ensureCli } from "./cli.js";
import "./converter.js";
import "./dialog.js";
import { handle } from "./ipc/handle.js";
import "./instances.js";
import "./legacy.js";
import "./mods.js";
import "./opener.js";
import "./path.js";
import "./platforms.js";
import { injectOs } from "./os.js";
import "./tray.js";
import "./updater.js";
import "./window.js";
import { activeChildren } from "./shell.js";

function tempestFilesFromArgs(args: string[], workingDirectory: string): string[] {
	return args.flatMap((argument) => {
		let filePath: string;
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

function queueAndNotifyOpenFiles(files: string[]): void {
	if (files.length === 0) return;
	queueOpenFiles(files);
	const window = getMainWindow();
	if (window && !window.isDestroyed() && !window.webContents.isLoading()) {
		window.webContents.send("open-mod-files", null);
	}
}

// tempest://install?mod=<id>&version=<v> links from the mod repository.
function deepLinksFromArgs(args: string[]): string[] {
	return args.filter((argument) => /^tempest:\/\//i.test(argument));
}

function queueAndNotifyDeepLinks(urls: string[]): void {
	if (urls.length === 0) return;
	queueDeepLinks(urls);
	const window = getMainWindow();
	if (window && !window.isDestroyed() && !window.webContents.isLoading()) {
		window.webContents.send("deep-link", null);
	}
}

queueOpenFiles(tempestFilesFromArgs(process.argv.slice(1), process.cwd()));
queueDeepLinks(deepLinksFromArgs(process.argv.slice(1)));

// Register the tempest:// scheme. Installers register it too (protocols: in
// electron-builder.yml); dev runs must do it themselves. In dev the electron
// binary needs the app path as an extra argument so the URL opens this app.
if (process.defaultApp) {
	app.setAsDefaultProtocolClient("tempest", process.execPath, [path.resolve(process.argv[1])]);
} else {
	app.setAsDefaultProtocolClient("tempest");
}

app.on("open-file", (event, filePath) => {
	event.preventDefault();
	queueAndNotifyOpenFiles(tempestFilesFromArgs([filePath], process.cwd()));
});

app.on("open-url", (event, url) => {
	event.preventDefault();
	queueAndNotifyDeepLinks(deepLinksFromArgs([url]));
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
		queueAndNotifyDeepLinks(deepLinksFromArgs(commandLine));
		showMainWindow();
	});
}

const loadURL = serve({ directory: "build" });

// Whether closing the window should hide it into the tray instead of
// quitting. The renderer pushes this via `app:set-close-behavior` whenever
// its background-work state changes (running games, active lobby, ...).
let hideOnClose = false;

handle("app:set-close-behavior", (_event, { hideToTray }) => {
	hideOnClose = hideToTray;
});

function createWindow(): void {
	const icon = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "resources", "icons", "icon.png");

	const window = new BrowserWindow({
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
	setMainWindow(window);

	window.webContents.on("did-finish-load", () => {
		injectOs(window);
	});

	window.webContents.on("will-navigate", (event, url) => {
		try {
			const u = new URL(url);
			// Packaged: only the app:// protocol electron-serve serves. Dev: exact
			// origin match (a prefix check would also let
			// http://localhost:1420.evil.com through).
			const sameOrigin = app.isPackaged
				? u.protocol === "app:"
				: `${u.protocol}//${u.host}` === "http://localhost:1420";
			if (!sameOrigin) event.preventDefault();
		} catch {
			event.preventDefault();
		}
	});

	// Never hand webview window requests (Ctrl/Cmd/Shift+click, middle-click,
	// target="_blank") to the system browser. External links are opened
	// explicitly from the renderer via openUrl().
	window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));

	if (!app.isPackaged) {
		void window.loadURL("http://localhost:1420");
	} else {
		void loadURL(window);
	}

	window.on("closed", () => {
		setMainWindow(null);
	});

	window.on("close", (e) => {
		if (hideOnClose) {
			e.preventDefault();
			window.hide();
		}
		// Otherwise fall through and close normally.
	});
}

app.whenReady()
	.then(() => {
		if (!hasInstanceLock) return;
		const upsert = (obj: Record<string, string[]>, key: string, value: string) => {
			const lower = key.toLowerCase();
			for (const k of Object.keys(obj)) {
				if (k.toLowerCase() === lower) {
					obj[k] = [value];
					return;
				}
			}
			obj[key] = [value];
		};

		session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
			const responseHeaders = details.responseHeaders ?? {};

			if (details.method === "OPTIONS") {
				upsert(responseHeaders, "Access-Control-Allow-Origin", "*");
				upsert(responseHeaders, "Access-Control-Allow-Methods", "POST, GET, OPTIONS");
				upsert(responseHeaders, "Access-Control-Allow-Headers", "*");
				callback({ responseHeaders, statusLine: "HTTP/1.1 204 No Content" });
				return;
			}

			upsert(
				responseHeaders,
				"Content-Security-Policy",
				"default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; img-src * data: blob:; media-src *; font-src * data:; connect-src *; frame-src *;",
			);
			upsert(responseHeaders, "Access-Control-Allow-Origin", "*");
			callback({ responseHeaders });
		});
		createWindow();
		// Make the CLI usable as `tempest <command>` from a terminal.
		ensureCli();
	})
	.catch(console.error);

app.on("window-all-closed", () => {
	app.quit();
});

// macOS: clicking the dock icon must re-show the window after it was hidden
// to the tray (Electron does not do this by default; Tauri's event loop did).
app.on("activate", () => {
	showMainWindow();
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
